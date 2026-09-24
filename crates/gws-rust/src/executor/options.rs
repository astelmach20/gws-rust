// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Request flags: parsing clap matches for a generated method command into
//! an [`Invocation`], and running it.
//!
//! The flag definitions live in `commands.rs`; the ids used here must match.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::{
    Credentials, ExecOptions, Invocation, ItemsMode, PaginationConfig, UploadMode, UploadSource,
    WaitConfig, execute, input,
};
use crate::discovery::{RestDescription, RestMethod};
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use crate::helpers::modelarmor::SanitizeConfig;

/// Set to `1` to never send `x-goog-user-project` (same as `--no-quota-project`).
pub const NO_QUOTA_PROJECT_ENV: &str = "GWSR_NO_QUOTA_PROJECT";

/// Parse a boolean environment variable strictly: unset/empty is false,
/// `1`/`true`/`yes`/`on` true, `0`/`false`/`no`/`off` false, anything else is
/// an error.
pub fn env_flag(name: &str) -> Result<bool, GwsError> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(GwsError::Validation(format!("{name} is not valid UTF-8")))
        }
        Ok(v) => parse_flag_value(name, &v),
    }
}

fn parse_flag_value(name: &str, v: &str) -> Result<bool, GwsError> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" | "0" | "false" | "no" | "off" => Ok(false),
        "1" | "true" | "yes" | "on" => Ok(true),
        other => Err(GwsError::Validation(format!(
            "{name} must be 1 or 0 (true/false), got {other:?}"
        ))),
    }
}

/// A typed flag value. Flags that a given method does not define read as
/// absent; a type mismatch is a programming error and is reported.
fn get<'a, T: std::any::Any + Clone + Send + Sync + 'static>(
    m: &'a clap::ArgMatches,
    id: &str,
) -> Result<Option<&'a T>, GwsError> {
    match m.try_get_one::<T>(id) {
        Ok(v) => Ok(v),
        Err(clap::parser::MatchesError::UnknownArgument { .. }) => Ok(None),
        Err(e) => Err(super::errors::other(anyhow::anyhow!(
            "internal error reading flag --{id}: {e}"
        ))),
    }
}

fn get_string<'a>(m: &'a clap::ArgMatches, id: &str) -> Result<Option<&'a str>, GwsError> {
    Ok(get::<String>(m, id)?.map(String::as_str))
}

fn get_flag(m: &clap::ArgMatches, id: &str) -> Result<bool, GwsError> {
    Ok(get::<bool>(m, id)?.copied().unwrap_or(false))
}

fn present(m: &clap::ArgMatches, id: &str) -> bool {
    // `Err` only means this method has no such flag.
    m.try_contains_id(id).unwrap_or(false)
}

/// Pagination flags. `--page-limit` defaults to 0 (unlimited).
pub fn parse_pagination_config(m: &clap::ArgMatches) -> Result<PaginationConfig, GwsError> {
    Ok(PaginationConfig {
        page_all: get_flag(m, "page-all")? || present(m, "page-items"),
        page_limit: get::<u32>(m, "page-limit")?.copied().unwrap_or(0),
        page_delay_ms: get::<u64>(m, "page-delay")?.copied().unwrap_or(100),
    })
}

/// Build [`ExecOptions`] from flags on top of the environment defaults.
pub fn parse_exec_options(m: &clap::ArgMatches) -> Result<ExecOptions, GwsError> {
    let mut opts = ExecOptions::from_env()?;
    opts.dry_run = get_flag(m, "dry-run")?;
    opts.fields = get_string(m, "fields")?.map(str::to_string);
    if present(m, "page-items") {
        opts.page_items = Some(match get_string(m, "page-items")? {
            Some(f) if !f.is_empty() => ItemsMode::Field(f.to_string()),
            _ => ItemsMode::Auto,
        });
    }
    if get_flag(m, "wait")? {
        let mut cfg = WaitConfig::default();
        if let Some(secs) = get::<u64>(m, "wait-timeout")?.copied() {
            cfg.timeout = Duration::from_secs(secs);
        }
        opts.wait = Some(cfg);
    }
    if let Some(raw) = get_string(m, "timeout")? {
        opts.retry.response_timeout = gws_rust_core::client::parse_timeout_secs(raw, "--timeout")?;
    }
    if get_flag(m, "no-quota-project")? {
        opts.quota_project = false;
    }
    if get_flag(m, "upload-resumable")? {
        opts.upload_mode = UploadMode::Resumable;
    }
    opts.decode_field = get_string(m, "decode-field")?.map(str::to_string);
    opts.allow_unknown_params = get_flag(m, "allow-unknown-params")?;
    opts.allow_unknown_fields = get_flag(m, "allow-unknown-fields")?;
    opts.assume_yes = get_flag(m, "yes")?;
    Ok(opts)
}

/// Resolve credentials for `method`: a token plus a provider that can refresh
/// it on 401. No stored credentials means an unauthenticated request.
async fn resolve_credentials(method: &RestMethod) -> Result<Credentials, GwsError> {
    let scopes: Vec<&str> = crate::select_scope(&method.scopes).into_iter().collect();
    match crate::auth::get_token(&scopes).await {
        Ok(token) => Ok(Credentials::Refreshable {
            token,
            provider: Arc::new(crate::auth::token_provider(&scopes)),
        }),
        Err(e) => {
            let msg = format!("{e:#}");
            // NB: matches the bail!() message in auth::load_credentials_inner.
            if msg.starts_with("No credentials found") {
                Ok(Credentials::None)
            } else {
                Err(GwsError::Auth(format!("Authentication failed: {msg}")))
            }
        }
    }
}

/// Run a generated method command from its clap matches.
pub async fn run_from_matches(
    doc: &RestDescription,
    method: &RestMethod,
    m: &clap::ArgMatches,
    sanitize: &SanitizeConfig,
    format: &OutputFormat,
) -> Result<(), GwsError> {
    let (params_text, body_text) =
        input::read_json_args(get_string(m, "params")?, get_string(m, "json")?).await?;
    let params = input::parse_params(params_text.as_deref())?;
    let body: Option<Value> = input::parse_body(body_text.as_deref())?;

    // Validate file paths before any I/O; use the canonical paths for I/O.
    let upload_path = get_string(m, "upload")?
        .map(|p| crate::validate::validate_safe_file_path(p, "--upload"))
        .transpose()?;
    let output_path: Option<PathBuf> = get_string(m, "output")?
        .map(|p| crate::validate::validate_safe_file_path(p, "--output"))
        .transpose()?;
    let upload_path_str = upload_path
        .as_deref()
        .map(|p| {
            p.to_str().ok_or_else(|| {
                GwsError::Validation(format!("--upload path {} is not valid UTF-8", p.display()))
            })
        })
        .transpose()?;
    let upload_content_type = get_string(m, "upload-content-type")?;
    let upload = upload_path_str.map(|path| UploadSource::File {
        path,
        content_type: upload_content_type,
    });

    let options = parse_exec_options(m)?;
    if options.decode_field.is_some() && output_path.is_none() {
        return Err(GwsError::Validation(
            "--decode-field requires -o/--output".to_string(),
        ));
    }
    // A dry run sends nothing, so it never needs (or touches) credentials.
    let credentials = if options.dry_run {
        Credentials::None
    } else {
        resolve_credentials(method).await?
    };

    execute(Invocation {
        doc,
        method,
        params,
        body,
        credentials,
        upload,
        output_path,
        pagination: parse_pagination_config(m)?,
        sanitize: sanitize.clone(),
        format: format.clone(),
        capture_output: false,
        options,
    })
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> clap::Command {
        let mut method = RestMethod {
            id: Some("drive.files.list".into()),
            http_method: "GET".into(),
            ..Default::default()
        };
        method.parameters.insert(
            "pageToken".into(),
            crate::discovery::MethodParameter {
                location: Some("query".into()),
                ..Default::default()
            },
        );
        let doc = RestDescription::default();
        clap::Command::new("t")
            .args(crate::commands::method_args(&doc, &method))
            .arg(
                clap::Arg::new("dry-run")
                    .long("dry-run")
                    .action(clap::ArgAction::SetTrue),
            )
    }

    #[test]
    fn pagination_defaults_to_unlimited() {
        let m = cmd().get_matches_from(["t", "--page-all"]);
        let p = parse_pagination_config(&m).unwrap();
        assert!(p.page_all);
        assert_eq!(p.page_limit, 0);
        assert_eq!(p.page_delay_ms, 100);
    }

    #[test]
    fn pagination_custom() {
        let m =
            cmd().get_matches_from(["t", "--page-all", "--page-limit", "20", "--page-delay", "5"]);
        let p = parse_pagination_config(&m).unwrap();
        assert_eq!((p.page_all, p.page_limit, p.page_delay_ms), (true, 20, 5));
    }

    #[test]
    #[serial_test::serial]
    fn page_items_implies_page_all() {
        let m = cmd().get_matches_from(["t", "--page-items"]);
        assert!(parse_pagination_config(&m).unwrap().page_all);
        let opts = parse_exec_options(&m).unwrap();
        assert_eq!(opts.page_items, Some(ItemsMode::Auto));
        let m = cmd().get_matches_from(["t", "--page-items=files"]);
        let opts = parse_exec_options(&m).unwrap();
        assert_eq!(opts.page_items, Some(ItemsMode::Field("files".into())));
    }

    #[test]
    #[serial_test::serial]
    fn exec_option_flags() {
        let m = cmd().get_matches_from([
            "t",
            "--fields",
            "files(id)",
            "--timeout",
            "0",
            "--no-quota-project",
            "--allow-unknown-params",
        ]);
        let o = parse_exec_options(&m).unwrap();
        assert_eq!(o.fields.as_deref(), Some("files(id)"));
        assert_eq!(o.retry.response_timeout, None);
        assert!(!o.quota_project);
        assert!(o.allow_unknown_params);
        let m = cmd().get_matches_from(["t", "--timeout", "abc"]);
        assert!(parse_exec_options(&m).is_err());
    }

    #[test]
    fn flag_values() {
        assert!(parse_flag_value("X", "1").unwrap());
        assert!(!parse_flag_value("X", "off").unwrap());
        assert!(parse_flag_value("X", "sure").is_err());
    }
}
