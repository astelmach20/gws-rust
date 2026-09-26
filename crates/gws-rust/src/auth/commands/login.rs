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

//! `gwsr auth login`.

use std::collections::HashSet;
use std::io::IsTerminal;
use std::time::Duration;

use serde_json::json;

use crate::auth::client_config;
use crate::auth::credentials::authorized_user_json;
use crate::auth::flow::{self, LoginOptions, RedirectMode};
use crate::auth::http::Endpoints;
use crate::auth::keystore::{Keystore, Purpose};
use crate::auth::profiles::{self, ProfileMetadata};
use crate::auth::scopes;
use crate::auth::token_cache::TokenCache;
use crate::error::GwsError;

/// First NDJSON event of `auth login`: the URL to open (for agents driving
/// the flow; the same URL is printed for humans on stderr).
pub(crate) fn url_event(url: &str, mode: &str, timeout_secs: u64) -> serde_json::Value {
    json!({
        "event": "authorization_url",
        "url": url,
        "mode": mode,
        "timeout_seconds": timeout_secs,
        "next": if mode == "manual" {
            "open the URL, approve, then write the redirected http://127.0.0.1 URL to stdin"
        } else {
            "open the URL in a browser on this machine and approve"
        },
    })
}

/// `auth login` writes NDJSON: one compact JSON object per line.
fn print_json_line(value: &serde_json::Value) -> Result<(), GwsError> {
    use std::io::Write;
    let line = serde_json::to_string(value)
        .map_err(|e| GwsError::Validation(format!("cannot serialize output: {e}")))?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")
        .and_then(|()| out.flush())
        .map_err(|e| GwsError::Validation(format!("cannot write output: {e}")))
}

/// Which preset to start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeMode {
    /// Read-only core scopes (interactive picker on a terminal).
    Default,
    Write,
    Full,
    Custom(Vec<String>),
}

pub(crate) struct LoginArgs {
    pub scope_mode: ScopeMode,
    pub services: Option<HashSet<String>>,
    pub open_browser: bool,
    pub mode: RedirectMode,
    pub timeout: Duration,
    pub login_hint: Option<String>,
}

pub(crate) fn parse_args(m: &clap::ArgMatches) -> Result<LoginArgs, GwsError> {
    let scope_mode = if let Some(list) = crate::args::value::<String>(m, "scopes")? {
        let scopes: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(scopes::normalize_scope)
            .collect();
        if scopes.is_empty() {
            return Err(GwsError::Validation(
                "--scopes needs at least one scope".into(),
            ));
        }
        ScopeMode::Custom(scopes)
    } else if crate::args::flag(m, "full")? {
        ScopeMode::Full
    } else if crate::args::flag(m, "write")? {
        ScopeMode::Write
    } else {
        ScopeMode::Default
    };
    let services = crate::args::value::<String>(m, "services")?.map(|v| {
        v.split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<HashSet<String>>()
    });
    let timeout = crate::args::value::<u64>(m, "timeout")?
        .copied()
        .ok_or_else(|| GwsError::Validation("--timeout is required".into()))?;
    Ok(LoginArgs {
        scope_mode,
        services,
        open_browser: !crate::args::flag(m, "no-browser")?,
        mode: if crate::args::flag(m, "no-localhost")? {
            RedirectMode::Manual
        } else {
            RedirectMode::Loopback
        },
        timeout: Duration::from_secs(timeout),
        login_hint: crate::args::value::<String>(m, "login-hint")?.cloned(),
    })
}

/// Run `auth login` from raw arguments (used by `auth setup --login`).
///
/// # Errors
///
/// See [`handle`].
pub async fn run_login(args: &[String]) -> Result<(), GwsError> {
    let Some(m) = super::parse_or_help(super::login_subcommand(), "login", args)? else {
        return Ok(());
    };
    handle(&m).await
}

fn preset(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// Resolve the scopes to request (before identity scopes are added).
async fn resolve_scopes(
    mode: ScopeMode,
    project_id: Option<&str>,
    services: Option<&HashSet<String>>,
    interactive: bool,
) -> Result<Vec<String>, GwsError> {
    let (base, readonly_only) = match mode {
        ScopeMode::Custom(scopes) => return Ok(scopes),
        ScopeMode::Full => (preset(scopes::FULL_SCOPES), false),
        ScopeMode::Write => (preset(scopes::WRITE_SCOPES), false),
        ScopeMode::Default => {
            if interactive && let Some(selected) = super::picker::pick(project_id, services).await?
            {
                return Ok(selected);
            }
            (preset(scopes::READONLY_SCOPES), true)
        }
    };
    let mut result = scopes::filter_scopes_by_services(base, services);
    if let Some(services) = services {
        super::picker::augment_with_discovery_scopes(&mut result, services, readonly_only).await;
    }
    if result.is_empty() {
        return Err(GwsError::Validation(
            "none of the requested services has usable OAuth scopes; check --services".into(),
        ));
    }
    Ok(result)
}

/// Final scope list: restrictive filtering, identity scopes, dedup.
pub(crate) fn finalize_scopes(scopes_in: Vec<String>) -> Vec<String> {
    let mut out = scopes::filter_redundant_restrictive_scopes(scopes_in);
    for s in scopes::IDENTITY_SCOPES {
        if !out.iter().any(|e| e == s) {
            out.push((*s).to_string());
        }
    }
    let mut seen = HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// Handle parsed `auth login` arguments.
///
/// # Errors
///
/// Missing OAuth client, key-storage problems (checked before the browser
/// opens), flow failures and write errors.
pub(crate) async fn handle(m: &clap::ArgMatches) -> Result<(), GwsError> {
    let args = parse_args(m)?;
    let client = client_config::resolve().map_err(crate::auth::to_gws_error)?;
    let (base, active, paths) =
        profiles::active_profile_paths().map_err(crate::auth::to_gws_error)?;

    // Fail before the browser opens if the encryption key is unusable.
    let backend = crate::env::get()?.keyring_backend.unwrap_or_default();
    let keystore = std::sync::Arc::new(Keystore::for_kind(&base, backend));
    {
        let ks = std::sync::Arc::clone(&keystore);
        tokio::task::spawn_blocking(move || -> Result<(), GwsError> {
            ks.key_for_encryption().map(|_| ()).map_err(GwsError::from)
        })
        .await
        .map_err(|e| GwsError::other(format!("key setup task failed: {e}")))??;
    }

    // The scope picker draws on stdout, so it needs a terminal there too.
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let stderr_tty = std::io::stderr().is_terminal();
    let requested = finalize_scopes(
        resolve_scopes(
            args.scope_mode,
            client.project_id.as_deref(),
            args.services.as_ref(),
            interactive,
        )
        .await?,
    );

    let endpoints = Endpoints::default();
    let opts = LoginOptions {
        scopes: requested.clone(),
        mode: args.mode,
        open_browser: args.open_browser && stderr_tty && args.mode == RedirectMode::Loopback,
        timeout: args.timeout,
        login_hint: args.login_hint,
    };
    let mode = match args.mode {
        RedirectMode::Loopback => "loopback",
        RedirectMode::Manual => "manual",
    };
    let timeout_secs = opts.timeout.as_secs();
    let on_url = |url: &str| -> Result<(), crate::auth::AuthError> {
        print_json_line(&url_event(url, mode, timeout_secs))
            .map_err(|e| crate::auth::AuthError::Internal(e.to_string()))
    };
    let tokens = flow::run(&client, &endpoints, &opts, &on_url).await?;

    let account = match flow::fetch_email(&tokens.access_token, &endpoints).await {
        Ok(email) => Some(email),
        Err(e) => {
            tracing::warn!("signed in, but could not look up the account email: {e:#}");
            None
        }
    };

    let json = authorized_user_json(
        &client.client_id,
        &client.client_secret,
        &tokens.refresh_token,
    );
    let meta = ProfileMetadata {
        account: account.clone(),
        client_id: client.client_id.clone(),
        granted_scopes: tokens.granted_scopes.clone(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    let save_paths = paths.clone();
    let ks = std::sync::Arc::clone(&keystore);
    tokio::task::spawn_blocking(move || -> Result<(), GwsError> {
        save_paths.ensure_dir().map_err(crate::auth::to_gws_error)?;
        let ct = ks.encrypt(Purpose::Credentials, json.as_bytes())?;
        crate::fs_util::atomic_write(&save_paths.credentials, &ct).map_err(|e| {
            GwsError::CredentialStore(format!(
                "cannot write '{}': {e}",
                save_paths.credentials.display()
            ))
        })?;
        profiles::save_metadata(&save_paths.metadata, &meta).map_err(crate::auth::to_gws_error)
    })
    .await
    .map_err(|e| GwsError::other(format!("save task failed: {e}")))??;

    // Access tokens cached for the previous grant are now stale. Clear them
    // under the cache lock so a concurrent refresh cannot write one back.
    TokenCache::new(
        &paths.token_cache,
        &paths.token_lock,
        std::sync::Arc::clone(&keystore),
    )
    .clear()
    .await?;

    let save_base = base.clone();
    let profile_name = active.name.clone();
    let made_active = tokio::task::spawn_blocking(move || -> Result<bool, GwsError> {
        let configured = crate::config::profile_in(&crate::config::config_path_in(&save_base))?;
        if configured.is_some() {
            Ok(false)
        } else {
            profiles::set_active_profile(&save_base, &profile_name)
                .map_err(crate::auth::to_gws_error)?;
            Ok(true)
        }
    })
    .await
    .map_err(|e| GwsError::other(format!("save task failed: {e}")))??;

    crate::timezone::invalidate_cache()?;

    let not_granted: Vec<&String> = requested
        .iter()
        .filter(|s| !tokens.granted_scopes.contains(s))
        .collect();
    if !not_granted.is_empty() {
        tracing::warn!(
            "these requested scopes were not granted (unchecked on the consent screen \
             or not allowed for this client): {}",
            not_granted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    print_json_line(&json!({
        "event": "login_complete",
        "status": "success",
        "profile": active.name,
        "active_profile_set": made_active,
        "account": account,
        "credentials_file": paths.credentials.display().to_string(),
        "encryption": {
            "algorithm": "AES-256-GCM",
            "key_backend": keystore.kind().as_str(),
            "key_location": keystore.describe(),
        },
        "client_source": client.source.kind(),
        "requested_scopes": requested,
        "granted_scopes": tokens.granted_scopes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> clap::ArgMatches {
        let mut v = vec!["login"];
        v.extend_from_slice(args);
        super::super::login_subcommand()
            .try_get_matches_from(v)
            .unwrap()
    }

    #[test]
    fn parse_defaults() {
        let a = parse_args(&matches(&[])).unwrap();
        assert_eq!(a.scope_mode, ScopeMode::Default);
        assert!(a.open_browser);
        assert_eq!(a.mode, RedirectMode::Loopback);
        assert_eq!(a.timeout, Duration::from_secs(300));
    }

    #[test]
    fn parse_flags() {
        let a = parse_args(&matches(&[
            "--scopes",
            "gmail.modify, https://mail.google.com/",
            "--no-browser",
            "--no-localhost",
            "--timeout",
            "30",
        ]))
        .unwrap();
        assert_eq!(
            a.scope_mode,
            ScopeMode::Custom(vec![
                "https://www.googleapis.com/auth/gmail.modify".into(),
                "https://mail.google.com/".into()
            ])
        );
        assert!(a.services.is_none());
        assert!(!a.open_browser);
        assert_eq!(a.mode, RedirectMode::Manual);
        assert_eq!(a.timeout, Duration::from_secs(30));
        assert!(parse_args(&matches(&["--scopes", " , "])).is_err());

        let a = parse_args(&matches(&["-s", "Gmail,drive", "--write"])).unwrap();
        assert_eq!(a.scope_mode, ScopeMode::Write);
        assert_eq!(
            a.services.unwrap(),
            ["gmail".to_string(), "drive".to_string()]
                .into_iter()
                .collect()
        );
    }

    #[tokio::test]
    async fn non_interactive_default_is_readonly() {
        let s = resolve_scopes(ScopeMode::Default, None, None, false)
            .await
            .unwrap();
        assert_eq!(s, preset(scopes::READONLY_SCOPES));
    }

    #[tokio::test]
    async fn services_filter_presets() {
        let services: HashSet<String> = ["gmail".to_string()].into_iter().collect();
        let s = resolve_scopes(ScopeMode::Write, None, Some(&services), false)
            .await
            .unwrap();
        assert_eq!(
            s,
            vec!["https://www.googleapis.com/auth/gmail.modify".to_string()]
        );
        let s = resolve_scopes(ScopeMode::Default, None, Some(&services), false)
            .await
            .unwrap();
        assert_eq!(
            s,
            vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()]
        );
        let s = resolve_scopes(ScopeMode::Full, None, Some(&services), false)
            .await
            .unwrap();
        assert!(s.contains(&"https://www.googleapis.com/auth/gmail.settings.basic".to_string()));
    }

    #[test]
    fn url_event_is_single_line_json() {
        let v = url_event(
            "https://accounts.google.com/o/oauth2/v2/auth?x=1",
            "manual",
            300,
        );
        let line = serde_json::to_string(&v).unwrap();
        assert!(!line.contains('\n'));
        let back: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(back["event"], "authorization_url");
        assert_eq!(
            back["url"],
            "https://accounts.google.com/o/oauth2/v2/auth?x=1"
        );
        assert_eq!(back["mode"], "manual");
    }

    #[test]
    fn finalize_adds_identity_and_dedups() {
        let s = finalize_scopes(vec![
            "https://www.googleapis.com/auth/gmail.metadata".into(),
            "https://www.googleapis.com/auth/gmail.readonly".into(),
            "openid".into(),
            "https://www.googleapis.com/auth/gmail.readonly".into(),
        ]);
        assert_eq!(
            s,
            vec![
                "https://www.googleapis.com/auth/gmail.readonly".to_string(),
                "openid".into(),
                "https://www.googleapis.com/auth/userinfo.email".into(),
                "https://www.googleapis.com/auth/userinfo.profile".into(),
            ]
        );
    }
}
