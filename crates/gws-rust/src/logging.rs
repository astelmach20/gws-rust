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

//! Structured Logging
//!
//! Diagnostics go to stderr and optionally to a JSON-lines
//! file with daily rotation. stdout stays reserved for command output.
//!
//! ## stderr filter, highest precedence first
//!
//! 1. `-v` / `-vv` / `-vvv` (info / debug / trace for gwsr) or `-q` (errors only)
//! 2. `GWSR_LOG` (a `tracing` filter directive such as `gwsr=debug`)
//! 3. `RUST_LOG`
//! 4. `log` in `config.toml`
//! 5. default: warnings and errors
//!
//! stderr lines are JSON objects unless `-v` is given or the output format is
//! a human one (table/yaml/csv).
//!
//! ## File logging
//!
//! `GWSR_LOG_FILE` (or `log_file` in `config.toml`) names a directory that
//! receives `gwsr.log.YYYY-MM-DD` JSON-line files at debug level. The
//! directory is created (or restricted) `0700` and files are created `0600` (the process
//! umask is `0077`). The returned [`LogGuard`] must be kept alive until the
//! process exits so buffered records are flushed.

use std::path::{Path, PathBuf};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use crate::error::GwsError;

/// Crates whose verbosity `-v` raises.
const OWN_TARGETS: &[&str] = &["gwsr", "gws_rust_core"];

/// Keeps the non-blocking file writer alive; dropping it flushes pending
/// records. Hold it for the lifetime of `main`.
#[must_use = "dropping the guard stops file logging and flushes it"]
pub struct LogGuard {
    _file: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Inputs to logging initialization (already resolved by `main`).
#[derive(Debug, Default, Clone)]
pub struct LogOptions {
    /// Net `-v` count minus `-q` (negative = quiet).
    pub verbosity: i8,
    /// `GWSR_LOG`.
    pub gwsr_log: Option<String>,
    /// `RUST_LOG`.
    pub rust_log: Option<String>,
    /// `log` from the config file.
    pub config_log: Option<String>,
    /// Log file directory (`GWSR_LOG_FILE` or config `log_file`).
    pub file_dir: Option<PathBuf>,
    /// Human-readable stderr lines instead of JSON lines (set for `-v` or a
    /// human output format).
    pub human: bool,
}

impl LogOptions {
    /// The environment-provided filters (already validated by
    /// [`crate::env`]).
    pub fn from_env(verbosity: i8, env: &crate::env::Env) -> Self {
        Self {
            verbosity,
            gwsr_log: env.log.clone(),
            rust_log: env.rust_log.clone(),
            ..Self::default()
        }
    }
}

fn own_directive(level: &str) -> String {
    let mut parts = vec!["warn".to_string()];
    parts.extend(OWN_TARGETS.iter().map(|t| format!("{t}={level}")));
    parts.join(",")
}

/// The stderr filter directive selected by the precedence rules.
pub fn stderr_directive(opts: &LogOptions) -> String {
    match opts.verbosity {
        v if v < 0 => "error".to_string(),
        1 => own_directive("info"),
        2 => own_directive("debug"),
        v if v >= 3 => own_directive("trace"),
        _ => opts
            .gwsr_log
            .clone()
            .or_else(|| opts.rust_log.clone())
            .or_else(|| opts.config_log.clone())
            .unwrap_or_else(|| "warn".to_string()),
    }
}

/// Every directive reaching here was validated with its source (environment
/// or config file); a failure is still a configuration error.
fn parse_filter(directive: &str) -> Result<EnvFilter, GwsError> {
    EnvFilter::try_new(directive)
        .map_err(|e| GwsError::Config(format!("invalid log filter '{directive}': {e}")))
}

/// Create the log directory, or tighten an existing one, to owner-only
/// permissions: debug logs name files, accounts and request URLs.
fn create_private_dir(dir: &Path) -> Result<(), GwsError> {
    crate::fs_util::ensure_private_dir(dir).map_err(|e| {
        GwsError::from(anyhow::Error::new(e).context(format!(
            "failed to create or restrict log directory {} to 0700",
            dir.display()
        )))
    })
}

/// Install the global tracing subscriber.
pub fn init(opts: &LogOptions) -> Result<LogGuard, GwsError> {
    let stderr_filter = parse_filter(&stderr_directive(opts))?;
    // Machine-readable by default: one JSON object per stderr line. `-v` or a
    // human output format switches to compact human lines.
    let (stderr_human, stderr_json) = if opts.human {
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(opts.verbosity >= 2)
            .with_ansi(crate::output::stderr_supports_color())
            .compact()
            .with_filter(stderr_filter);
        (Some(layer), None)
    } else {
        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_ansi(false)
            .with_filter(stderr_filter);
        (None, Some(layer))
    };

    let (file_layer, guard) = match &opts.file_dir {
        Some(dir) => {
            create_private_dir(dir)?;
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("gwsr.log")
                .build(dir)
                .map_err(|e| {
                    GwsError::from(
                        anyhow::Error::new(e)
                            .context(format!("failed to open log file in {}", dir.display())),
                    )
                })?;
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .with_writer(writer)
                .with_target(true)
                .with_filter(parse_filter(&own_directive("debug"))?);
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    tracing_subscriber::registry()
        .with(stderr_human)
        .with(stderr_json)
        .with(file_layer)
        .try_init()
        .map_err(|e| {
            GwsError::from(anyhow::anyhow!("failed to install the log subscriber: {e}"))
        })?;

    Ok(LogGuard { _file: guard })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(verbosity: i8) -> LogOptions {
        LogOptions {
            verbosity,
            ..Default::default()
        }
    }

    #[test]
    fn default_is_warn() {
        assert_eq!(stderr_directive(&opts(0)), "warn");
    }

    #[test]
    fn verbosity_flags_win_over_env_and_config() {
        let o = LogOptions {
            verbosity: 2,
            gwsr_log: Some("gwsr=trace".into()),
            rust_log: Some("info".into()),
            config_log: Some("error".into()),
            file_dir: None,
            human: false,
        };
        assert_eq!(stderr_directive(&o), "warn,gwsr=debug,gws_rust_core=debug");
        assert_eq!(
            stderr_directive(&opts(1)),
            "warn,gwsr=info,gws_rust_core=info"
        );
        assert_eq!(
            stderr_directive(&opts(5)),
            "warn,gwsr=trace,gws_rust_core=trace"
        );
        assert_eq!(stderr_directive(&opts(-1)), "error");
    }

    #[test]
    fn env_precedence_gwsr_log_then_rust_log_then_config() {
        let mut o = LogOptions {
            gwsr_log: Some("gwsr=debug".into()),
            rust_log: Some("info".into()),
            config_log: Some("error".into()),
            ..Default::default()
        };
        assert_eq!(stderr_directive(&o), "gwsr=debug");
        o.gwsr_log = None;
        assert_eq!(stderr_directive(&o), "info");
        o.rust_log = None;
        assert_eq!(stderr_directive(&o), "error");
    }

    #[test]
    fn invalid_filter_is_a_config_error() {
        let err = parse_filter("gwsr=notalevel").unwrap_err();
        assert!(matches!(err, GwsError::Config(_)), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn log_dir_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs/nested");
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn existing_log_dir_is_tightened_to_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn log_dir_that_is_a_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let err = create_private_dir(&file).unwrap_err();
        assert!(err.to_string().contains("log directory"), "{err}");
    }
}
