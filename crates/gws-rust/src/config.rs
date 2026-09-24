// Copyright 2026 The gws-rust Authors
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

//! User configuration file: `<config dir>/config.toml`
//! (`~/.config/gwsr/config.toml` unless `GWSR_CONFIG_DIR` is set).
//!
//! Precedence for every setting: command-line flag > environment variable >
//! config file > built-in default. Flags are applied by `main`; this module
//! combines the validated environment ([`crate::env::Env`]) with the file.
//! Unknown keys and invalid values are errors, never ignored.
//!
//! ```toml
//! format = "table"          # json | table | yaml | csv          (GWSR_FORMAT)
//! json_style = "auto"       # auto | compact | pretty            (GWSR_JSON_STYLE)
//! page_limit = 50           # default --page-limit               (GWSR_PAGE_LIMIT)
//! page_delay_ms = 100       # default --page-delay               (GWSR_PAGE_DELAY_MS)
//! sanitize_template = "projects/p/locations/l/templates/t"     # (GWSR_SANITIZE_TEMPLATE)
//! sanitize_mode = "warn"    # warn | block                       (GWSR_SANITIZE_MODE)
//! profile = "work"          # default credential profile, written by `gwsr auth use`;
//!                           # --profile and GWSR_PROFILE take precedence (read by auth)
//! timeout_secs = 60         # HTTP request timeout; 0 disables   (GWSR_TIMEOUT; the
//!                           # executor's --timeout flag wins over both)
//! log = "gwsr=info"         # stderr log filter                  (GWSR_LOG, RUST_LOG)
//! log_file = "/var/log/gwsr" # JSON log directory                (GWSR_LOG_FILE)
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::env::Env;
use crate::error::GwsError;
use crate::formatter::{FORMAT_NAMES, OutputFormat};
use crate::helpers::modelarmor::SanitizeMode;

/// File name inside the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// JSON layout preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsonStylePref {
    /// Pretty on a terminal, compact otherwise.
    #[default]
    Auto,
    /// Always compact.
    Compact,
    /// Always pretty.
    Pretty,
}

impl JsonStylePref {
    /// Parse `auto`, `compact` or `pretty`; anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(JsonStylePref::Auto),
            "compact" => Some(JsonStylePref::Compact),
            "pretty" => Some(JsonStylePref::Pretty),
            _ => None,
        }
    }
}

/// Raw contents of `config.toml`.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    format: Option<String>,
    json_style: Option<String>,
    page_limit: Option<u32>,
    page_delay_ms: Option<u64>,
    sanitize_template: Option<String>,
    sanitize_mode: Option<String>,
    profile: Option<String>,
    timeout_secs: Option<u64>,
    log: Option<String>,
    log_file: Option<PathBuf>,
}

/// Settings after applying environment > config file > defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    /// Default output format.
    pub format: OutputFormat,
    /// JSON layout.
    pub json_style: JsonStylePref,
    /// Default `--page-limit` (None = executor default).
    pub page_limit: Option<u32>,
    /// Default `--page-delay` in ms (None = executor default).
    pub page_delay_ms: Option<u64>,
    /// Default Model Armor template.
    pub sanitize_template: Option<String>,
    /// Model Armor mode.
    pub sanitize_mode: SanitizeMode,
    /// HTTP request timeout (`GWSR_TIMEOUT` > `timeout_secs` > 60 s; `None`
    /// disables it). The executor's `--timeout` flag overrides it per
    /// command.
    pub request_timeout: Option<Duration>,
    /// stderr log filter from the config file (env handled by logging).
    pub log: Option<String>,
    /// Log file directory (`GWSR_LOG_FILE` or config).
    pub log_file: Option<PathBuf>,
}

fn invalid(source: &str, key: &str, value: &str, expected: &str) -> GwsError {
    GwsError::Config(format!(
        "invalid {key} '{value}' in {source}: expected {expected}"
    ))
}

fn parse_format(value: &str, source: &str) -> Result<OutputFormat, GwsError> {
    OutputFormat::parse(value)
        .map_err(|_| invalid(source, "format", value, &FORMAT_NAMES.join(" | ")))
}

fn parse_json_style(value: &str, source: &str) -> Result<JsonStylePref, GwsError> {
    JsonStylePref::parse(value)
        .ok_or_else(|| invalid(source, "json_style", value, "auto | compact | pretty"))
}

fn parse_sanitize_mode(value: &str, source: &str) -> Result<SanitizeMode, GwsError> {
    SanitizeMode::parse(value)
        .ok_or_else(|| invalid(source, "sanitize_mode", value, "warn | block"))
}

/// Parse the text of a config file. `origin` names it in error messages.
pub fn parse_config(text: &str, origin: &Path) -> Result<ConfigFile, GwsError> {
    toml::from_str(text).map_err(|e| {
        GwsError::Config(format!(
            "invalid config file {}: {}",
            origin.display(),
            e.to_string().trim_end()
        ))
    })
}

/// Load `path` if it exists. A missing file is not an error; an unreadable
/// or invalid one is.
pub fn load_config_file(path: &Path) -> Result<Option<ConfigFile>, GwsError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text, path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(GwsError::Config(format!(
            "cannot read config file {}: {e}",
            path.display()
        ))),
    }
}

/// Resolve settings: the validated environment `env` > the config `file`
/// (named `origin` in errors) > built-in defaults. Pure, so tests pass an
/// [`Env`] built with [`Env::from_vars`].
pub fn resolve(file: Option<&ConfigFile>, origin: &Path, env: &Env) -> Result<Settings, GwsError> {
    let default_file = ConfigFile::default();
    let file = file.unwrap_or(&default_file);
    let origin = origin.display().to_string();

    let format = match (env.format, &file.format) {
        (Some(v), _) => v,
        (None, Some(v)) => parse_format(v, &origin)?,
        (None, None) => OutputFormat::default(),
    };
    let json_style = match (env.json_style, &file.json_style) {
        (Some(v), _) => v,
        (None, Some(v)) => parse_json_style(v, &origin)?,
        (None, None) => JsonStylePref::default(),
    };
    let sanitize_mode = match (&env.sanitize_mode, &file.sanitize_mode) {
        (Some(v), _) => v.clone(),
        (None, Some(v)) => parse_sanitize_mode(v, &origin)?,
        (None, None) => SanitizeMode::default(),
    };
    let request_timeout = match (env.timeout, file.timeout_secs) {
        (Some(v), _) => v,
        (None, Some(secs)) => (secs > 0).then(|| Duration::from_secs(secs)),
        (None, None) => Some(gws_rust_core::client::DEFAULT_TIMEOUT),
    };
    let log = file.log.clone();
    if let Some(directive) = &log {
        crate::env::check_log_filter(directive).map_err(|e| {
            invalid(
                &origin,
                "log",
                directive,
                &format!("a tracing filter directive ({e})"),
            )
        })?;
    }

    Ok(Settings {
        format,
        json_style,
        page_limit: env.page_limit.or(file.page_limit),
        page_delay_ms: env.page_delay_ms.or(file.page_delay_ms),
        sanitize_template: env
            .sanitize_template
            .clone()
            .or_else(|| file.sanitize_template.clone()),
        sanitize_mode,
        request_timeout,
        log,
        log_file: env.log_file.clone().or_else(|| file.log_file.clone()),
    })
}

/// Path of the config file inside the config directory `base`.
pub fn config_path_in(base: &Path) -> PathBuf {
    base.join(CONFIG_FILE_NAME)
}

/// Path of the user config file.
pub fn config_path() -> Result<PathBuf, GwsError> {
    let base = crate::auth::try_config_dir().map_err(crate::auth::to_gws_error)?;
    Ok(config_path_in(&base))
}

/// The `profile` key of the config file at `path`, if the file and key exist.
/// Read by auth, which ranks it below `--profile` and `GWSR_PROFILE`.
pub fn profile_in(path: &Path) -> Result<Option<String>, GwsError> {
    Ok(load_config_file(path)?.and_then(|f| f.profile))
}

/// Set the `profile` key of the config file at `path` (used by
/// `gwsr auth use`), preserving every other key and comment. The existing
/// file must be valid; it is replaced atomically.
pub fn set_profile_in(path: &Path, name: &str) -> Result<(), GwsError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(GwsError::Config(format!(
                "cannot read config file {}: {e}",
                path.display()
            )));
        }
    };
    // Refuse to rewrite a file that would not load afterwards.
    parse_config(&text, path)?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| GwsError::Config(format!("invalid config file {}: {e}", path.display())))?;
    doc["profile"] = toml_edit::value(name);
    crate::fs_util::atomic_write(path, doc.to_string().as_bytes()).map_err(|e| {
        GwsError::other(
            anyhow::Error::new(e).context(format!("cannot write config file {}", path.display())),
        )
    })
}

/// Load and resolve settings from the validated environment and the config
/// file.
pub fn load() -> Result<Settings, GwsError> {
    let path = config_path()?;
    let file = load_config_file(&path)?;
    resolve(file.as_ref(), &path, crate::env::get()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> Env {
        Env::from_vars(pairs.iter().copied()).unwrap()
    }

    fn origin() -> PathBuf {
        PathBuf::from("/cfg/config.toml")
    }

    #[test]
    fn defaults_without_file_or_env() {
        let s = resolve(None, &origin(), &env_of(&[])).unwrap();
        assert_eq!(
            s,
            Settings {
                request_timeout: Some(gws_rust_core::client::DEFAULT_TIMEOUT),
                ..Settings::default()
            }
        );
    }

    #[test]
    fn file_values_are_used() {
        let file = parse_config(
            r#"
format = "table"
json_style = "compact"
page_limit = 50
page_delay_ms = 0
sanitize_template = "projects/p/locations/l/templates/t"
sanitize_mode = "block"
profile = "work"
timeout_secs = 30
log = "gwsr=info"
log_file = "/tmp/logs"
"#,
            &origin(),
        )
        .unwrap();
        let s = resolve(Some(&file), &origin(), &env_of(&[])).unwrap();
        assert_eq!(s.format, OutputFormat::Table);
        assert_eq!(s.json_style, JsonStylePref::Compact);
        assert_eq!(s.page_limit, Some(50));
        assert_eq!(s.page_delay_ms, Some(0));
        assert_eq!(s.sanitize_mode, SanitizeMode::Block);
        assert_eq!(s.log.as_deref(), Some("gwsr=info"));
        assert_eq!(s.log_file, Some(PathBuf::from("/tmp/logs")));
    }

    #[test]
    fn env_overrides_file() {
        let file = parse_config("format = \"table\"\npage_limit = 5\n", &origin()).unwrap();
        let s = resolve(
            Some(&file),
            &origin(),
            &env_of(&[("GWSR_FORMAT", "csv"), ("GWSR_PAGE_LIMIT", "7")]),
        )
        .unwrap();
        assert_eq!(s.format, OutputFormat::Csv);
        assert_eq!(s.page_limit, Some(7));
        // Empty env values count as unset.
        let s = resolve(Some(&file), &origin(), &env_of(&[("GWSR_FORMAT", "")])).unwrap();
        assert_eq!(s.format, OutputFormat::Table);
    }

    #[test]
    fn timeout_precedence_env_then_config_then_default() {
        let file = parse_config("timeout_secs = 30\n", &origin()).unwrap();
        let s = resolve(Some(&file), &origin(), &env_of(&[("GWSR_TIMEOUT", "5")])).unwrap();
        assert_eq!(s.request_timeout, Some(Duration::from_secs(5)));
        let s = resolve(Some(&file), &origin(), &env_of(&[("GWSR_TIMEOUT", "0")])).unwrap();
        assert_eq!(s.request_timeout, None);
        let s = resolve(Some(&file), &origin(), &env_of(&[])).unwrap();
        assert_eq!(s.request_timeout, Some(Duration::from_secs(30)));
        let file = parse_config("timeout_secs = 0\n", &origin()).unwrap();
        let s = resolve(Some(&file), &origin(), &env_of(&[])).unwrap();
        assert_eq!(s.request_timeout, None);
        let s = resolve(None, &origin(), &env_of(&[])).unwrap();
        assert_eq!(
            s.request_timeout,
            Some(gws_rust_core::client::DEFAULT_TIMEOUT)
        );
    }

    #[test]
    fn set_profile_preserves_other_keys_and_rejects_invalid_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = config_path_in(tmp.path());
        assert_eq!(profile_in(&path).unwrap(), None);
        set_profile_in(&path, "work").unwrap();
        assert_eq!(profile_in(&path).unwrap().as_deref(), Some("work"));

        std::fs::write(&path, "# keep me\nformat = \"table\"\nprofile = \"old\"\n").unwrap();
        set_profile_in(&path, "home").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep me"), "{text}");
        assert!(text.contains("format = \"table\""), "{text}");
        assert_eq!(profile_in(&path).unwrap().as_deref(), Some("home"));

        std::fs::write(&path, "formt = 1\n").unwrap();
        assert!(set_profile_in(&path, "x").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "formt = 1\n");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = parse_config("formt = \"table\"\n", &origin()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/cfg/config.toml"), "{msg}");
        assert!(msg.contains("formt"), "{msg}");
    }

    #[test]
    fn invalid_values_are_rejected_with_their_source() {
        let file = parse_config("format = \"xml\"\n", &origin()).unwrap();
        let err = resolve(Some(&file), &origin(), &env_of(&[])).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid format 'xml' in /cfg/config.toml"),
            "{err}"
        );

        let file = parse_config("sanitize_mode = \"loud\"\n", &origin()).unwrap();
        let err = resolve(Some(&file), &origin(), &env_of(&[])).unwrap_err();
        assert!(err.to_string().contains("warn | block"), "{err}");

        let file = parse_config("page_limit = -1\n", &origin());
        assert!(file.is_err());

        for bad in ["gwsr=nope", "bogus!!"] {
            let file = parse_config(&format!("log = \"{bad}\"\n"), &origin()).unwrap();
            let err = resolve(Some(&file), &origin(), &env_of(&[])).unwrap_err();
            assert!(matches!(err, GwsError::Config(_)), "{bad}: {err:?}");
        }
    }

    #[test]
    fn missing_file_is_not_an_error_but_unreadable_is() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            load_config_file(&tmp.path().join("nope.toml"))
                .unwrap()
                .is_none()
        );
        // A directory where the file should be cannot be read.
        let dir = tmp.path().join("config.toml");
        std::fs::create_dir(&dir).unwrap();
        assert!(load_config_file(&dir).is_err());
    }
}
