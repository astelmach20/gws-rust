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

//! Environment variables: the one registry of every variable `gwsr` reads at
//! run time, and the validated snapshot the rest of the binary consumes.
//!
//! [`REGISTRY`] lists each variable with its help text and accepted values.
//! [`Env::from_vars`] parses a set of variables into typed values; `main`
//! forces [`get`] right after the shell-completion hook, so every variable is
//! validated before any command runs, whether or not the command uses it.
//! Nothing else in the binary (or in `gws-rust-core`, which never reads the
//! environment) calls `std::env::var` for these names.
//!
//! Rules, all failing with [`GwsError::Config`] (exit 8):
//!
//! - every value must be valid UTF-8 and match the variable's grammar;
//! - an empty or whitespace-only value counts as unset;
//! - an unknown `GWSR_*` name is an error with a "did you mean" suggestion,
//!   because it is almost always a typo or a removed name. There are no
//!   aliases for old names.
//!
//! The build-time variables `GWSR_DEFAULT_CLIENT_ID` /
//! `GWSR_DEFAULT_CLIENT_SECRET` are read by the compiler (`option_env!`), not
//! at run time. They are tolerated in the run-time environment (a shell that
//! built `gwsr` usually still exports them) and otherwise ignored.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use gws_rust_core::validate::{EndpointPolicy, PathPolicy};
use reqwest::Url;
use secrecy::SecretString;

use crate::auth::keystore::BackendKind;
use crate::config::JsonStylePref;
use crate::error::GwsError;
use crate::formatter::{FORMAT_NAMES, OutputFormat};
use crate::helpers::modelarmor::{ModelArmorTemplate, SanitizeMode};

/// One environment variable `gwsr` reads.
#[derive(Debug, Clone, Copy)]
pub struct EnvVar {
    /// Variable name.
    pub name: &'static str,
    /// One-line description for `gwsr --help`.
    pub help: &'static str,
    /// The accepted values, quoted in error messages.
    pub accepted: &'static str,
    /// Never echo the value in error messages.
    pub secret: bool,
}

const fn var(name: &'static str, help: &'static str, accepted: &'static str) -> EnvVar {
    EnvVar {
        name,
        help,
        accepted,
        secret: false,
    }
}

const fn secret(name: &'static str, help: &'static str, accepted: &'static str) -> EnvVar {
    EnvVar {
        name,
        help,
        accepted,
        secret: true,
    }
}

const BOOL: &str =
    "1, true, yes or on to enable; 0, false, no or off to disable (case-insensitive)";
const ABSOLUTE_DIR: &str = "an absolute directory path";
const FILE_PATH: &str = "a file path without control characters";

/// Every environment variable `gwsr` reads at run time, in help order.
pub const REGISTRY: &[EnvVar] = &[
    secret(
        "GWSR_TOKEN",
        "Pre-obtained OAuth2 access token (highest priority)",
        "an access token: printable ASCII without spaces",
    ),
    var(
        "GWSR_TOKEN_FILE",
        "File holding a pre-obtained access token",
        FILE_PATH,
    ),
    var(
        "GWSR_CREDENTIALS_FILE",
        "Path to an OAuth or service-account credentials JSON file",
        FILE_PATH,
    ),
    var(
        "GWSR_PROFILE",
        "Credential profile (same as --profile)",
        "a profile name: 1-64 letters, digits, '-', '_' or '.', not starting with '.'",
    ),
    var(
        "GWSR_IMPERSONATE",
        "Service accounts: user to act as (same as --impersonate)",
        "a user email address such as user@example.com",
    ),
    var(
        "GWSR_CLIENT_ID",
        "OAuth client ID (for gwsr auth login)",
        "an OAuth client ID: printable ASCII without spaces",
    ),
    secret(
        "GWSR_CLIENT_SECRET",
        "OAuth client secret (for gwsr auth login)",
        "an OAuth client secret: printable ASCII without spaces",
    ),
    var(
        "GWSR_CONFIG_DIR",
        "Config directory, absolute (default: ~/.config/gwsr)",
        ABSOLUTE_DIR,
    ),
    var(
        "GWSR_CACHE_DIR",
        "Cache directory, absolute (default: platform cache dir + /gwsr)",
        ABSOLUTE_DIR,
    ),
    var(
        "GWSR_KEYRING_BACKEND",
        "Keyring backend: keyring (default) or file",
        "keyring or file",
    ),
    var(
        "GWSR_PROJECT_ID",
        "GCP project for quota and billing",
        "a GCP project ID (6-30 lowercase letters, digits or '-', starting with a letter, \
         optionally prefixed by 'domain:') or a project number",
    ),
    var(
        "GWSR_NO_QUOTA_PROJECT",
        "1: never send x-goog-user-project (same as --no-quota-project)",
        BOOL,
    ),
    var(
        "GWSR_TIMEOUT",
        "Request timeout in seconds (default 60; 0 disables; same as --timeout)",
        "a whole number of seconds; 0 disables the timeout",
    ),
    var(
        "GWSR_REQUIRE_CONFIRM",
        "1: also require --yes for sending, sharing and running scripts",
        BOOL,
    ),
    var(
        "GWSR_RESTRICT_PATHS",
        "cwd: confine file flags (--output, --upload, ...) to the current directory",
        "cwd",
    ),
    var(
        "GWSR_API_BASE_URL",
        "Trusted API endpoint override (https, or http on localhost)",
        "an https:// URL (http:// only for localhost) without credentials, query or fragment",
    ),
    var(
        "GWSR_FORMAT",
        "Default output format (json, table, yaml, csv)",
        "json, table, yaml, yml or csv",
    ),
    var(
        "GWSR_JSON_STYLE",
        "JSON layout: auto (default), compact, pretty",
        "auto, compact or pretty",
    ),
    var(
        "GWSR_PAGE_LIMIT",
        "Default --page-limit",
        "a non-negative whole number (0 = unlimited)",
    ),
    var(
        "GWSR_PAGE_DELAY_MS",
        "Default --page-delay",
        "a non-negative whole number of milliseconds",
    ),
    var(
        "GWSR_SANITIZE_TEMPLATE",
        "Default Model Armor template (--sanitize)",
        "projects/PROJECT/locations/LOCATION/templates/TEMPLATE",
    ),
    var(
        "GWSR_SANITIZE_MODE",
        "Sanitization mode: warn (default) or block",
        "warn or block",
    ),
    var(
        "GWSR_LOG",
        "stderr log filter, e.g. gwsr=debug (overrides RUST_LOG)",
        LOG_FILTER,
    ),
    var(
        "RUST_LOG",
        "stderr log filter when GWSR_LOG is unset",
        LOG_FILTER,
    ),
    var(
        "GWSR_LOG_FILE",
        "Directory for JSON log files, absolute (daily rotation, mode 0600)",
        ABSOLUTE_DIR,
    ),
    var(
        crate::completions::COMPLETE_VAR,
        "Set by the shell completion script; do not set manually",
        "any value; set only by the scripts from `gwsr completions`",
    ),
];

const LOG_FILTER: &str = "a tracing filter: comma-separated LEVEL or TARGET[=LEVEL] directives, \
     where LEVEL is trace, debug, info, warn, error or off and TARGET is a module path such as \
     gwsr or gws_rust_core::client";

/// Compile-time variables (`option_env!`). Ignored at run time.
const BUILD_TIME_VARS: &[&str] = &["GWSR_DEFAULT_CLIENT_ID", "GWSR_DEFAULT_CLIENT_SECRET"];

/// Prefix that marks a variable as belonging to `gwsr`.
const PREFIX: &str = "GWSR_";

/// Validated values of every variable in [`REGISTRY`]. Unset (or empty)
/// variables are `None` / `false` / the documented default.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub token: Option<SecretString>,
    pub token_file: Option<PathBuf>,
    pub credentials_file: Option<PathBuf>,
    pub profile: Option<String>,
    pub impersonate: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<SecretString>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub keyring_backend: Option<BackendKind>,
    pub project_id: Option<String>,
    pub no_quota_project: bool,
    /// `Some(None)`: the timeout is disabled (`0`).
    pub timeout: Option<Option<Duration>>,
    pub require_confirm: bool,
    pub restrict_paths: PathPolicy,
    pub api_base_url: Option<Url>,
    pub format: Option<OutputFormat>,
    pub json_style: Option<JsonStylePref>,
    pub page_limit: Option<u32>,
    pub page_delay_ms: Option<u64>,
    pub sanitize_template: Option<String>,
    pub sanitize_mode: Option<SanitizeMode>,
    pub log: Option<String>,
    pub rust_log: Option<String>,
    pub log_file: Option<PathBuf>,
}

impl Env {
    /// Parse and validate `vars` (name/value pairs, e.g.
    /// `std::env::vars_os()`). Variables outside `gwsr`'s namespace are
    /// ignored, except `RUST_LOG`.
    ///
    /// # Errors
    ///
    /// [`GwsError::Config`] listing every invalid value and unknown `GWSR_*`
    /// name, each with the accepted values or a suggestion.
    pub fn from_vars<I, K, V>(vars: I) -> Result<Self, GwsError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        let mut env = Env::default();
        let mut problems = Vec::new();
        for (name, value) in vars {
            let (name, value): (OsString, OsString) = (name.into(), value.into());
            let Some(name) = name.to_str() else {
                if name.as_encoded_bytes().starts_with(PREFIX.as_bytes()) {
                    problems.push(format!(
                        "environment variable name {} is not valid UTF-8 and is not one gwsr \
                         reads; run `gwsr --help` for the supported variables",
                        name.to_string_lossy()
                    ));
                }
                continue;
            };
            let Some(entry) = lookup(name) else {
                if name.starts_with(PREFIX) && !BUILD_TIME_VARS.contains(&name) {
                    problems.push(unknown_message(name));
                }
                continue;
            };
            if let Err(problem) = env.apply(entry, &value) {
                problems.push(problem);
            }
        }
        if problems.is_empty() {
            Ok(env)
        } else {
            problems.sort();
            Err(GwsError::Config(problems.join(". ")))
        }
    }

    /// Validate one variable and store its value.
    fn apply(&mut self, entry: &EnvVar, raw: &OsString) -> Result<(), String> {
        let invalid = |problem: &str| {
            let shown = if entry.secret {
                format!("{} (value hidden)", entry.name)
            } else {
                format!("{}={:?}", entry.name, raw.to_string_lossy())
            };
            format!(
                "invalid environment variable {shown}: {problem}; expected {}",
                entry.accepted
            )
        };
        let Some(text) = raw.to_str() else {
            return Err(invalid("the value is not valid UTF-8"));
        };
        if entry.name == crate::completions::COMPLETE_VAR {
            // Consumed by the completion hook, which runs before validation.
            return Ok(());
        }
        let value = text.trim();
        if value.is_empty() {
            return Ok(());
        }
        let value = value.to_string();
        match entry.name {
            "GWSR_TOKEN" => {
                check_printable(&value).map_err(|p| invalid(&p))?;
                self.token = Some(SecretString::from(value));
            }
            "GWSR_TOKEN_FILE" => {
                self.token_file = Some(parse_file(&value).map_err(|p| invalid(&p))?)
            }
            "GWSR_CREDENTIALS_FILE" => {
                self.credentials_file = Some(parse_file(&value).map_err(|p| invalid(&p))?);
            }
            "GWSR_PROFILE" => {
                crate::auth::profiles::validate_profile_name(&value)
                    .map_err(|e| invalid(&format!("{e:#}")))?;
                self.profile = Some(value);
            }
            "GWSR_IMPERSONATE" => {
                check_email(&value).map_err(|p| invalid(&p))?;
                self.impersonate = Some(value);
            }
            "GWSR_CLIENT_ID" => {
                check_printable(&value).map_err(|p| invalid(&p))?;
                self.client_id = Some(value);
            }
            "GWSR_CLIENT_SECRET" => {
                check_printable(&value).map_err(|p| invalid(&p))?;
                self.client_secret = Some(SecretString::from(value));
            }
            "GWSR_CONFIG_DIR" => {
                self.config_dir = Some(parse_dir(&value).map_err(|p| invalid(&p))?)
            }
            "GWSR_CACHE_DIR" => self.cache_dir = Some(parse_dir(&value).map_err(|p| invalid(&p))?),
            "GWSR_LOG_FILE" => self.log_file = Some(parse_dir(&value).map_err(|p| invalid(&p))?),
            "GWSR_KEYRING_BACKEND" => {
                self.keyring_backend = Some(
                    BackendKind::parse(&value).ok_or_else(|| invalid("unknown keyring backend"))?,
                );
            }
            "GWSR_PROJECT_ID" => {
                if !is_gcp_project(&value) {
                    return Err(invalid("not a GCP project ID or number"));
                }
                self.project_id = Some(value);
            }
            "GWSR_NO_QUOTA_PROJECT" => {
                self.no_quota_project = parse_bool(&value).map_err(|p| invalid(&p))?
            }
            "GWSR_REQUIRE_CONFIRM" => {
                self.require_confirm = parse_bool(&value).map_err(|p| invalid(&p))?
            }
            "GWSR_TIMEOUT" => {
                let secs: u64 = value
                    .parse()
                    .map_err(|_| invalid("not a whole number of seconds"))?;
                self.timeout = Some((secs > 0).then(|| Duration::from_secs(secs)));
            }
            "GWSR_RESTRICT_PATHS" => {
                self.restrict_paths =
                    PathPolicy::parse(Some(&value)).map_err(|_| invalid("unknown path policy"))?;
            }
            "GWSR_API_BASE_URL" => {
                self.api_base_url = gws_rust_core::validate::parse_api_base_override(Some(&value))
                    .map_err(|e| invalid(&e.to_string()))?;
            }
            "GWSR_FORMAT" => {
                self.format = Some(OutputFormat::parse(&value).map_err(|_| {
                    invalid(&format!(
                        "unknown output format (one of {})",
                        FORMAT_NAMES.join(", ")
                    ))
                })?);
            }
            "GWSR_JSON_STYLE" => {
                self.json_style = Some(
                    JsonStylePref::parse(&value).ok_or_else(|| invalid("unknown JSON style"))?,
                );
            }
            "GWSR_PAGE_LIMIT" => {
                self.page_limit = Some(
                    value
                        .parse()
                        .map_err(|_| invalid("not a non-negative whole number"))?,
                );
            }
            "GWSR_PAGE_DELAY_MS" => {
                self.page_delay_ms = Some(
                    value
                        .parse()
                        .map_err(|_| invalid("not a non-negative whole number"))?,
                );
            }
            "GWSR_SANITIZE_TEMPLATE" => {
                ModelArmorTemplate::parse(&value).map_err(|e| invalid(&e.to_string()))?;
                self.sanitize_template = Some(value);
            }
            "GWSR_SANITIZE_MODE" => {
                self.sanitize_mode = Some(
                    SanitizeMode::parse(&value).ok_or_else(|| invalid("unknown sanitize mode"))?,
                );
            }
            "GWSR_LOG" => {
                check_log_filter(&value).map_err(|p| invalid(&p))?;
                self.log = Some(value);
            }
            "RUST_LOG" => {
                check_log_filter(&value).map_err(|p| invalid(&p))?;
                self.rust_log = Some(value);
            }
            other => {
                return Err(format!(
                    "internal error: environment variable {other} is registered without a parser"
                ));
            }
        }
        Ok(())
    }

    /// The endpoint trust policy from `GWSR_API_BASE_URL`.
    pub fn endpoint_policy(&self) -> EndpointPolicy {
        EndpointPolicy::new(self.api_base_url.clone())
    }
}

fn lookup(name: &str) -> Option<&'static EnvVar> {
    REGISTRY.iter().find(|v| v.name == name)
}

/// The error for an unknown `GWSR_*` name, with the closest registered name.
fn unknown_message(name: &str) -> String {
    let hint = match suggest(name) {
        Some(s) => format!("did you mean {s}? "),
        None => String::new(),
    };
    format!(
        "unknown environment variable {name}: gwsr does not read it; {hint}Unset it, or run \
         `gwsr --help` for the supported variables"
    )
}

/// The registered `GWSR_*` name closest to `name`: the one sharing the most
/// `_`-separated words (so `GWSR_TIMEOUT_SECS` suggests `GWSR_TIMEOUT` and
/// `GWSR_CONFIRM_DESTRUCTIVE` suggests `GWSR_REQUIRE_CONFIRM`), ties and
/// word-less typos broken by Jaro-Winkler similarity.
pub fn suggest(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    let stem = upper.strip_prefix(PREFIX).unwrap_or(&upper);
    let words: Vec<&str> = stem.split('_').filter(|w| !w.is_empty()).collect();
    REGISTRY
        .iter()
        .filter(|v| v.name.starts_with(PREFIX) && v.name != crate::completions::COMPLETE_VAR)
        .map(|v| {
            let candidate = &v.name[PREFIX.len()..];
            let shared = candidate.split('_').filter(|w| words.contains(w)).count();
            (shared, strsim::jaro_winkler(stem, candidate), v.name)
        })
        .filter(|(shared, similarity, _)| *shared > 0 || *similarity >= 0.85)
        .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, _, name)| name)
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err("not a boolean".to_string()),
    }
}

fn check_printable(value: &str) -> Result<(), String> {
    if value.chars().all(|c| c.is_ascii_graphic()) {
        Ok(())
    } else {
        Err("contains whitespace, control or non-ASCII characters".to_string())
    }
}

fn parse_file(value: &str) -> Result<PathBuf, String> {
    gws_rust_core::validate::reject_dangerous_chars(value, "the path")
        .map_err(|e| e.to_string())?;
    Ok(PathBuf::from(value))
}

fn parse_dir(value: &str) -> Result<PathBuf, String> {
    let path = parse_file(value)?;
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("the path is not absolute".to_string())
    }
}

fn check_email(value: &str) -> Result<(), String> {
    let valid = match value.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && value.chars().all(|c| c.is_ascii_graphic())
        }
        None => false,
    };
    if valid {
        Ok(())
    } else {
        Err("not an email address".to_string())
    }
}

/// A GCP project ID (optionally domain-scoped, `example.com:my-project`) or
/// project number.
fn is_gcp_project(value: &str) -> bool {
    let id = match value.rsplit_once(':') {
        Some((domain, id)) => {
            let domain_ok = !domain.is_empty()
                && domain
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-.".contains(&b));
            if !domain_ok {
                return false;
            }
            id
        }
        None => value,
    };
    crate::helpers::modelarmor::is_project_id(id)
}

/// Validate a `tracing` filter strictly. `EnvFilter` itself accepts any word
/// as a target, so a typo like `debgu` or `gwsr!` would silently log nothing;
/// targets must be module paths.
pub(crate) fn check_log_filter(value: &str) -> Result<(), String> {
    tracing_subscriber::EnvFilter::try_new(value).map_err(|e| e.to_string())?;
    for directive in split_top_level(value) {
        let directive = directive.trim();
        if directive.is_empty() {
            return Err("empty directive".to_string());
        }
        // The target is everything before a span filter `[...]` or `=LEVEL`.
        let end = directive.find(['[', '=']).unwrap_or(directive.len());
        let target = &directive[..end];
        if !target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-' | '.'))
        {
            return Err(format!(
                "invalid target {target:?} in directive {directive:?}"
            ));
        }
    }
    Ok(())
}

/// Split a filter on commas that are not inside `[...]` or `{...}`.
fn split_top_level(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in value.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

static PROCESS: OnceLock<Result<Env, String>> = OnceLock::new();

/// The validated process environment, parsed on first use. `main` calls this
/// before dispatch, so later calls cannot fail in a real run.
///
/// # Errors
///
/// [`GwsError::Config`] if any variable is invalid or unknown.
pub fn get() -> Result<&'static Env, GwsError> {
    PROCESS
        .get_or_init(|| Env::from_vars(std::env::vars_os()).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|message| GwsError::Config(message.clone()))
}

#[cfg(test)]
mod tests {
    // Absolute paths differ by platform (`/etc` is not absolute on Windows).
    #[cfg(not(windows))]
    const ABS_CONFIG_DIR: &str = "/etc/gwsr";
    #[cfg(windows)]
    const ABS_CONFIG_DIR: &str = r"C:\ProgramData\gwsr";
    #[cfg(not(windows))]
    const ABS_CACHE_DIR: &str = "/var/cache/gwsr";
    #[cfg(windows)]
    const ABS_CACHE_DIR: &str = r"C:\ProgramData\gwsr\cache";
    #[cfg(not(windows))]
    const ABS_LOG_DIR: &str = "/var/log/gwsr";
    #[cfg(windows)]
    const ABS_LOG_DIR: &str = r"C:\ProgramData\gwsr\logs";

    use super::*;
    use secrecy::ExposeSecret;

    fn parse(pairs: &[(&str, &str)]) -> Result<Env, GwsError> {
        Env::from_vars(pairs.iter().map(|(k, v)| (*k, *v)))
    }

    fn err(pairs: &[(&str, &str)]) -> String {
        match parse(pairs) {
            Ok(env) => panic!("{pairs:?} was accepted: {env:?}"),
            Err(GwsError::Config(message)) => message,
            Err(other) => panic!("{pairs:?}: expected a config error, got {other:?}"),
        }
    }

    /// A valid and an invalid value for every registered variable.
    /// `GWSR_COMPLETE` accepts anything, so it has no invalid value.
    const CASES: &[(&str, &str, Option<&str>)] = &[
        ("GWSR_TOKEN", "ya29.token", Some("tok en")),
        ("GWSR_TOKEN_FILE", "token.txt", Some("tok\u{7}en")),
        ("GWSR_CREDENTIALS_FILE", "/k/key.json", Some("key\n.json")),
        ("GWSR_PROFILE", "work", Some("bogus!!")),
        ("GWSR_IMPERSONATE", "user@example.com", Some("bogus!!")),
        (
            "GWSR_CLIENT_ID",
            "123-abc.apps.googleusercontent.com",
            Some("a b"),
        ),
        ("GWSR_CLIENT_SECRET", "GOCSPX-secret", Some("sec ret")),
        ("GWSR_CONFIG_DIR", ABS_CONFIG_DIR, Some("relative/dir")),
        ("GWSR_CACHE_DIR", ABS_CACHE_DIR, Some("bogus!!")),
        ("GWSR_KEYRING_BACKEND", "file", Some("bogus!!")),
        ("GWSR_PROJECT_ID", "my-project-1", Some("bogus!!")),
        ("GWSR_NO_QUOTA_PROJECT", "1", Some("bogus!!")),
        ("GWSR_TIMEOUT", "30", Some("bogus!!")),
        ("GWSR_REQUIRE_CONFIRM", "true", Some("bogus!!")),
        ("GWSR_RESTRICT_PATHS", "cwd", Some("bogus!!")),
        (
            "GWSR_API_BASE_URL",
            "https://proxy.example/",
            Some("bogus!!"),
        ),
        ("GWSR_FORMAT", "table", Some("bogus!!")),
        ("GWSR_JSON_STYLE", "pretty", Some("bogus!!")),
        ("GWSR_PAGE_LIMIT", "5", Some("bogus!!")),
        ("GWSR_PAGE_DELAY_MS", "0", Some("bogus!!")),
        (
            "GWSR_SANITIZE_TEMPLATE",
            "projects/my-project/locations/us-central1/templates/t1",
            Some("bogus!!"),
        ),
        ("GWSR_SANITIZE_MODE", "block", Some("bogus!!")),
        ("GWSR_LOG", "gwsr=debug,warn", Some("bogus!!")),
        ("RUST_LOG", "info", Some("bogus!!")),
        ("GWSR_LOG_FILE", ABS_LOG_DIR, Some("bogus!!")),
        ("GWSR_COMPLETE", "bash", None),
    ];

    #[test]
    fn cases_cover_the_registry_exactly() {
        let registry: Vec<&str> = REGISTRY.iter().map(|v| v.name).collect();
        let cases: Vec<&str> = CASES.iter().map(|c| c.0).collect();
        assert_eq!(registry, cases);
    }

    #[test]
    fn every_variable_accepts_its_valid_value() {
        for (name, valid, _) in CASES {
            parse(&[(name, valid)]).unwrap_or_else(|e| panic!("{name}={valid}: {e}"));
        }
    }

    #[test]
    fn every_invalid_value_is_a_config_error_naming_the_variable_and_accepted_values() {
        for (name, _, invalid) in CASES {
            let Some(invalid) = invalid else { continue };
            let message = err(&[(name, invalid)]);
            let entry = lookup(name).unwrap();
            assert!(message.contains(name), "{name}: {message}");
            assert!(message.contains(entry.accepted), "{name}: {message}");
            if entry.secret {
                assert!(
                    !message.contains(invalid),
                    "{name} leaked its value: {message}"
                );
            } else {
                assert!(
                    message.contains(&format!("{invalid:?}")),
                    "{name}: {message}"
                );
            }
            let error = GwsError::Config(message);
            assert_eq!(error.exit_code(), 8);
        }
    }

    #[test]
    fn empty_values_are_unset() {
        let env = parse(&[
            ("GWSR_TIMEOUT", ""),
            ("GWSR_FORMAT", "  "),
            ("GWSR_PROFILE", ""),
        ])
        .unwrap();
        assert_eq!(env.timeout, None);
        assert_eq!(env.format, None);
        assert_eq!(env.profile, None);
    }

    #[test]
    fn non_utf8_values_are_config_errors() {
        #[cfg(unix)]
        let bad = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0x66, 0xff, 0x6f])
        };
        // An unpaired surrogate is the Windows equivalent of invalid UTF-8.
        #[cfg(windows)]
        let bad = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x66, 0xD800, 0x6f])
        };
        let result = Env::from_vars([(OsString::from("GWSR_CONFIG_DIR"), bad)]);
        match result {
            Err(GwsError::Config(message)) => {
                assert!(message.contains("GWSR_CONFIG_DIR"), "{message}");
                assert!(message.contains("not valid UTF-8"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn values_are_typed() {
        let env = parse(&[
            ("GWSR_TIMEOUT", "0"),
            ("GWSR_PAGE_LIMIT", "7"),
            ("GWSR_FORMAT", "CSV"),
            ("GWSR_NO_QUOTA_PROJECT", "yes"),
            ("GWSR_REQUIRE_CONFIRM", "Off"),
            ("GWSR_RESTRICT_PATHS", "cwd"),
            ("GWSR_KEYRING_BACKEND", "file"),
            ("GWSR_TOKEN", "abc"),
            ("GWSR_API_BASE_URL", "http://localhost:8080"),
        ])
        .unwrap();
        assert_eq!(env.timeout, Some(None));
        assert_eq!(env.page_limit, Some(7));
        assert_eq!(env.format, Some(OutputFormat::Csv));
        assert!(env.no_quota_project);
        assert!(!env.require_confirm);
        assert_eq!(env.restrict_paths, PathPolicy::Cwd);
        assert_eq!(env.keyring_backend, Some(BackendKind::File));
        assert_eq!(env.token.as_ref().unwrap().expose_secret(), "abc");
        assert_eq!(
            env.api_base_url.as_ref().map(Url::as_str),
            Some("http://localhost:8080/")
        );
    }

    #[test]
    fn unknown_gwsr_variables_fail_with_a_suggestion() {
        let message = err(&[("GWSR_TIMEOUT_SECS", "30")]);
        assert!(
            message.contains("unknown environment variable GWSR_TIMEOUT_SECS"),
            "{message}"
        );
        assert!(message.contains("did you mean GWSR_TIMEOUT?"), "{message}");

        let message = err(&[("GWSR_CONFIRM_DESTRUCTIVE", "1")]);
        assert!(
            message.contains("did you mean GWSR_REQUIRE_CONFIRM?"),
            "{message}"
        );

        assert_eq!(suggest("GWSR_LOGFILE"), Some("GWSR_LOG_FILE"));
        assert_eq!(suggest("GWSR_FORMATT"), Some("GWSR_FORMAT"));
        assert_eq!(suggest("GWSR_XYZZY"), None);
        let message = err(&[("GWSR_XYZZY", "1")]);
        assert!(!message.contains("did you mean"), "{message}");
    }

    #[test]
    fn all_problems_are_reported_together() {
        let message = err(&[("GWSR_TIMEOUT", "soon"), ("GWSR_TIMEOUT_SECS", "1")]);
        assert!(message.contains("GWSR_TIMEOUT=\"soon\""), "{message}");
        assert!(message.contains("GWSR_TIMEOUT_SECS"), "{message}");
    }

    #[test]
    fn foreign_and_build_time_variables_are_ignored() {
        parse(&[
            ("HOME", "/home/u"),
            ("GOOGLE_APPLICATION_CREDENTIALS", "x"),
            ("GWSR_DEFAULT_CLIENT_ID", "id"),
            ("GWSR_DEFAULT_CLIENT_SECRET", "secret"),
        ])
        .unwrap();
    }

    #[test]
    fn log_filters_are_strict() {
        for ok in [
            "warn",
            "gwsr=debug",
            "gwsr=debug,gws_rust_core::client=trace",
            "info,[request]=debug",
            "gwsr[span{field=1}]=trace",
        ] {
            check_log_filter(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for bad in ["bogus!!", "gwsr=notalevel", "gwsr=debug,,info", "gw sr"] {
            assert!(check_log_filter(bad).is_err(), "{bad}");
        }
    }

    /// The names `GWSR_*` and `RUST_LOG` mentioned in `text`.
    fn mentioned(text: &str) -> std::collections::BTreeSet<String> {
        let mut names = std::collections::BTreeSet::new();
        for prefix in [PREFIX, "RUST_LOG"] {
            for (at, _) in text.match_indices(prefix) {
                let rest = &text[at..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                    .unwrap_or(rest.len());
                names.insert(rest[..end].to_string());
            }
        }
        names
    }

    fn registry_names() -> std::collections::BTreeSet<String> {
        REGISTRY.iter().map(|v| v.name.to_string()).collect()
    }

    /// The text between `start` and the next line starting with `end`.
    fn section<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
        let from = text
            .find(start)
            .unwrap_or_else(|| panic!("no {start:?} section"));
        let body = &text[from + start.len()..];
        let to = body.find(&format!("\n{end}")).unwrap_or(body.len());
        &body[..to]
    }

    #[test]
    fn help_environment_section_matches_the_registry() {
        let help = crate::cli_args::after_help();
        let listed = section(&help, "\nEnvironment:\n", "\n");
        assert_eq!(mentioned(listed), registry_names(), "{listed}");
    }

    #[test]
    fn readme_environment_table_matches_the_registry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
        let readme = std::fs::read_to_string(&path).unwrap();
        let table: String = section(&readme, "\n## Environment variables\n", "## ")
            .lines()
            .filter(|line| line.starts_with('|'))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(mentioned(&table), registry_names(), "{table}");
    }

    #[test]
    fn project_ids() {
        for ok in ["my-project", "123456789012", "example.com:my-project"] {
            assert!(is_gcp_project(ok), "{ok}");
        }
        for bad in ["short", "My-Project", "proj-", ":my-project", "a b"] {
            assert!(!is_gcp_project(bad), "{bad}");
        }
    }
}
