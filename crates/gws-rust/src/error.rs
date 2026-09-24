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

//! Structured error types and CLI error reporting.
//!
//! Core error types are re-exported from the `gws_rust_core` library crate.
//!
//! # Error output contract
//!
//! * Every failure writes exactly one error to **stderr**; **stdout** stays
//!   clean. The TTY state never changes what is written.
//! * By default (output format `json`) the error is a single-line JSON object:
//!   `{"error":{"code":403,"message":"…","reason":"…","retryable":false,"hint":"…"}}`.
//!   `hint` and `enable_url` are present only when known.
//! * With a human output format (`--format table|yaml|csv`, `GWSR_FORMAT` or
//!   `format` in config.toml) the error is human-readable instead
//!   (`error[kind]: …` plus a `hint:` line; colour only on a terminal).
//! * The process exit code identifies the error class (see
//!   [`EXIT_CODE_DOCUMENTATION`]).

use std::sync::Mutex;

use serde_json::{Value, json};

pub use gws_rust_core::error::*;

use crate::output::{colorize, sanitize_for_terminal};

/// Human-readable exit code table, keyed by (code, description).
///
/// Rendered into the top-level `--help` so the documentation cannot drift
/// from the constants.
pub const EXIT_CODE_DOCUMENTATION: &[(i32, &str)] = &[
    (0, "Success"),
    (
        GwsError::EXIT_CODE_API,
        "API error      — Google rejected the request (permanent; do not retry unchanged)",
    ),
    (
        GwsError::EXIT_CODE_AUTH,
        "Auth error     — no usable credentials, rejected/expired token, refused scopes or denied consent",
    ),
    (
        GwsError::EXIT_CODE_VALIDATION,
        "Validation     — bad arguments, flags or input",
    ),
    (
        GwsError::EXIT_CODE_DISCOVERY,
        "Discovery      — could not fetch or parse the API schema",
    ),
    (
        GwsError::EXIT_CODE_OTHER,
        "Internal       — unexpected failure (I/O, serialization, ...)",
    ),
    (
        GwsError::EXIT_CODE_API_RETRYABLE,
        "API retryable  — rate limit (429/403 rateLimitExceeded) or server error (5xx); retry with backoff",
    ),
    (
        GwsError::EXIT_CODE_CONFIRMATION_REQUIRED,
        "Not confirmed  — destructive or gated action needs --yes (or a terminal to confirm); nothing was sent",
    ),
    (
        GwsError::EXIT_CODE_CONFIG,
        "Config         — invalid or missing configuration (env vars, config.toml, profile, OAuth client file)",
    ),
    (
        GwsError::EXIT_CODE_CREDENTIAL_STORE,
        "Cred. store    — OS keyring / key file or stored credentials unreadable, unwritable or undecryptable",
    ),
    (
        GwsError::EXIT_CODE_NETWORK,
        "Network        — no response (connection, DNS, TLS, timeout); a non-idempotent call may have been applied",
    ),
];

/// Any failure that ends a `gwsr` invocation.
#[derive(Debug)]
pub enum CliError {
    /// A domain error from the library, helpers or executor.
    Gws(GwsError),
    /// An argument-parsing error from clap (already carries usage and tips).
    Clap(clap::Error),
}

impl From<GwsError> for CliError {
    fn from(e: GwsError) -> Self {
        CliError::Gws(e)
    }
}

impl From<clap::Error> for CliError {
    fn from(e: clap::Error) -> Self {
        CliError::Clap(e)
    }
}

impl CliError {
    /// Stable process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Clap(_) => GwsError::EXIT_CODE_VALIDATION,
            CliError::Gws(e) => e.exit_code(),
        }
    }
}

/// Facts about the command that failed, used to make hints specific.
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    /// Service alias the user typed (e.g. `gmail`).
    pub service: Option<String>,
    /// OAuth scopes the called method accepts (any one suffices).
    pub required_scopes: Vec<String>,
    /// Scopes the active credentials were granted, when known.
    pub granted_scopes: Option<Vec<String>>,
}

static CONTEXT: Mutex<Option<ErrorContext>> = Mutex::new(None);

/// Record context for error hints. Called by `main` once the target
/// service/method is known.
pub fn set_context(ctx: ErrorContext) {
    let mut guard = CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(ctx);
}

fn current_context() -> ErrorContext {
    CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Google reasons that mean the token lacks a required OAuth scope.
const SCOPE_REASONS: &[&str] = &[
    "insufficientPermissions",
    "ACCESS_TOKEN_SCOPE_INSUFFICIENT",
    "insufficientScopes",
];

/// Remedy for a failure, keyed on Google's error `reason`.
pub fn hint_for(err: &GwsError, ctx: &ErrorContext) -> Option<String> {
    match err {
        GwsError::Api {
            code,
            message,
            reason,
            enable_url,
        } => api_hint(*code, message, reason, enable_url.as_deref(), ctx),
        GwsError::ConfirmationRequired(_) => Some(
            "Re-run with --yes to confirm, or with --dry-run to preview the request. \
             Exit code 7 marks unconfirmed actions."
                .to_string(),
        ),
        GwsError::Auth(_) => Some(auth_hint(ctx)),
        GwsError::Config(_) => Some(
            "Fix the setting named above (environment variable, config.toml, profile, OAuth \
             client or credentials file). Exit code 8 marks configuration errors."
                .to_string(),
        ),
        GwsError::CredentialStore(_) => Some(
            "The local credential store failed. Check that the OS keyring is unlocked and \
             reachable (or set GWSR_KEYRING_BACKEND=file), and that the config directory is \
             readable and writable. Exit code 9 marks credential-store errors."
                .to_string(),
        ),
        GwsError::Network(_) => Some(
            "No response was received. Check the network connection and proxy settings, and \
             raise --timeout for slow links. Before re-running a create or other \
             non-idempotent call, check whether it was applied. Exit code 10 marks network \
             failures."
                .to_string(),
        ),
        GwsError::Validation(_) | GwsError::Discovery(_) | GwsError::Other(_) => None,
        #[allow(unreachable_patterns)] // GwsError is #[non_exhaustive] in core
        _ => None,
    }
}

/// Whether `err` is an API error caused by a missing OAuth scope.
fn is_scope_error(err: &CliError) -> bool {
    matches!(
        err,
        CliError::Gws(GwsError::Api { code, message, reason, .. })
            if lacks_scope(*code, message, reason)
    )
}

fn lacks_scope(code: u16, message: &str, reason: &str) -> bool {
    SCOPE_REASONS.contains(&reason)
        || (code == 403 && message.contains("insufficient authentication scopes"))
}

fn api_hint(
    code: u16,
    message: &str,
    reason: &str,
    enable_url: Option<&str>,
    ctx: &ErrorContext,
) -> Option<String> {
    if lacks_scope(code, message, reason) {
        return Some(scope_hint(ctx));
    }
    match reason {
        "accessNotConfigured" | "SERVICE_DISABLED" => Some(match enable_url {
            Some(url) => format!(
                "The API is not enabled for your GCP project. Enable it at {url} , \
                 wait a minute, then retry."
            ),
            None => "The API is not enabled for your GCP project. Enable it in the GCP \
                     Console (APIs & Services → Library), wait a minute, then retry."
                .to_string(),
        }),
        "rateLimitExceeded" | "userRateLimitExceeded" | "RATE_LIMIT_EXCEEDED" => Some(
            "Rate limited by Google. Retry after an exponential backoff; for bulk work \
             add --page-delay or lower concurrency. Exit code 6 marks retryable errors."
                .to_string(),
        ),
        "quotaExceeded" | "dailyLimitExceeded" => Some(
            "A usage quota is exhausted. Retrying immediately will fail again; wait for \
             the quota to reset or request more quota in the GCP Console."
                .to_string(),
        ),
        "failedPrecondition" | "FAILED_PRECONDITION" | "conditionNotMet" => Some(
            "The resource is not in a state that allows this operation (for example it \
             was modified concurrently, is in trash, or a prerequisite is missing). Fetch \
             the current state and adjust the request; retrying unchanged will fail again."
                .to_string(),
        ),
        _ if code == 429 => Some(
            "Rate limited by Google. Retry after an exponential backoff. Exit code 6 \
             marks retryable errors."
                .to_string(),
        ),
        _ if code == 401 => Some(
            "The access token was rejected. Sign in again with `gwsr auth login`, or \
             check GWSR_TOKEN if you set it."
                .to_string(),
        ),
        _ if code >= 500 => Some(
            "Google returned a server error. It is usually transient: retry after a \
             backoff (exit code 6 marks retryable errors)."
                .to_string(),
        ),
        _ => None,
    }
}

fn scope_hint(ctx: &ErrorContext) -> String {
    let mut hint = String::from("Your credentials lack an OAuth scope this method needs.");
    if ctx.required_scopes.is_empty() {
        hint.push_str(" Required scopes: see `gwsr schema <service>.<resource>.<method>`.");
    } else {
        hint.push_str(" Required (any one of): ");
        hint.push_str(&ctx.required_scopes.join(", "));
        hint.push('.');
    }
    match &ctx.granted_scopes {
        Some(granted) if !granted.is_empty() => {
            hint.push_str(" Granted: ");
            hint.push_str(&granted.join(", "));
            hint.push('.');
        }
        _ => hint.push_str(" Run `gwsr auth status` to see the scopes you granted."),
    }
    hint.push_str(" Fix: `");
    hint.push_str(&login_fix(ctx));
    hint.push_str("`.");
    hint
}

/// The `gwsr auth login` command that grants what the failed call needed.
fn login_fix(ctx: &ErrorContext) -> String {
    match (ctx.required_scopes.first(), ctx.service.as_deref()) {
        (Some(scope), _) => crate::auth::scopes::login_command_hint(std::slice::from_ref(scope)),
        (None, Some(service)) => format!("gwsr auth login -s {service}"),
        (None, None) => "gwsr auth login -s <service>".to_string(),
    }
}

/// Hint for an authentication failure. The message itself names the specific
/// fix for classified failures (expired grant, refused scopes, missing
/// profile); the hint adds how to inspect and repair the credentials.
fn auth_hint(ctx: &ErrorContext) -> String {
    format!(
        "Check the active profile and its granted scopes with `gwsr auth status`; sign in \
         again with `{}`. Exit code 2 marks authentication failures.",
        login_fix(ctx)
    )
}

/// Error kind label used in the human-readable form.
fn kind_label(err: &CliError) -> &'static str {
    match err {
        CliError::Clap(_) => "validation",
        CliError::Gws(e) => match e {
            GwsError::Api { .. } => "api",
            GwsError::Auth(_) => "auth",
            GwsError::Validation(_) => "validation",
            GwsError::Discovery(_) => "discovery",
            GwsError::ConfirmationRequired(_) => "confirmation",
            GwsError::Config(_) => "config",
            GwsError::CredentialStore(_) => "credential-store",
            GwsError::Network(_) => "network",
            GwsError::Other(_) => "internal",
            #[allow(unreachable_patterns)] // GwsError is #[non_exhaustive] in core
            _ => "internal",
        },
    }
}

/// Drop a leading `error: ` that some wrapped messages (clap renderings)
/// already carry, so output never reads `error[validation]: error: …`.
fn strip_error_prefix(msg: &str) -> &str {
    msg.trim_start()
        .strip_prefix("error: ")
        .unwrap_or(msg)
        .trim()
}

/// The machine-readable envelope for `err` (always a single line when
/// serialized compactly).
pub fn error_envelope(err: &CliError, ctx: &ErrorContext) -> Value {
    match err {
        CliError::Clap(e) => {
            // The error text up to the first blank line (usage/tips follow).
            let rendered = e.render().to_string();
            let summary: Vec<&str> = rendered
                .lines()
                .take_while(|l| !l.trim().is_empty())
                .map(str::trim)
                .collect();
            json!({
                "error": {
                    "code": 400,
                    "message": strip_error_prefix(&summary.join(" ")),
                    "reason": "validationError",
                    "retryable": false,
                }
            })
        }
        CliError::Gws(e) => {
            let mut envelope = e.to_json();
            if let Some(obj) = envelope.get_mut("error").and_then(Value::as_object_mut) {
                if let Some(Value::String(m)) = obj.get("message") {
                    let stripped = strip_error_prefix(m).to_string();
                    obj.insert("message".into(), Value::String(stripped));
                }
                obj.insert("retryable".into(), Value::Bool(e.is_retryable()));
                if let Some(hint) = hint_for(e, ctx) {
                    obj.insert("hint".into(), Value::String(hint));
                }
            }
            envelope
        }
    }
}

/// The human-readable report (without trailing newline).
pub fn human_report(err: &CliError, ctx: &ErrorContext, color: bool) -> String {
    match err {
        CliError::Clap(e) => {
            // clap already prints `error: …` plus usage and tips.
            let rendered = if color {
                e.render().ansi().to_string()
            } else {
                e.render().to_string()
            };
            rendered.trim_end().to_string()
        }
        CliError::Gws(e) => {
            let label = format!("error[{}]:", kind_label(err));
            let label = if color {
                crate::output::colorize(&label, "31")
            } else {
                label
            };
            let mut message = sanitize_for_terminal(strip_error_prefix(&e.to_string()));
            if let GwsError::Api { code, reason, .. } = e {
                message.push_str(&format!(
                    " (HTTP {code}, reason: {})",
                    sanitize_for_terminal(reason)
                ));
            }
            let mut out = format!("{label} {message}");
            if let Some(hint) = hint_for(e, ctx) {
                let prefix = if color {
                    colorize("hint:", "36")
                } else {
                    "hint:".to_string()
                };
                out.push_str(&format!("\n{prefix} {}", sanitize_for_terminal(&hint)));
            }
            out
        }
    }
}

/// Render `err` as it is written to stderr: one JSON line, or the human
/// report when `human` is set.
pub fn render(err: &CliError, ctx: &ErrorContext, human: bool, color: bool) -> String {
    if human {
        return human_report(err, ctx, color);
    }
    match serde_json::to_string(&error_envelope(err, ctx)) {
        Ok(line) => line,
        Err(e) => json!({
            "error": {
                "code": 500,
                "message": format!("failed to serialize error: {e}"),
                "reason": "internalError",
                "retryable": false,
            }
        })
        .to_string(),
    }
}

/// Report `err` on stderr following the module-level output contract.
/// `human` selects the human-readable form (a non-JSON output format).
pub fn report(err: &CliError, human: bool) {
    let mut ctx = current_context();
    if ctx.granted_scopes.is_none() && is_scope_error(err) {
        // Best effort: the hint says to run `gwsr auth status` when unknown.
        match crate::auth::granted_scopes() {
            Ok(granted) => ctx.granted_scopes = granted,
            Err(e) => tracing::debug!(error = %format!("{e:#}"), "granted scopes unavailable"),
        }
    }
    let color = crate::output::stderr_supports_color();
    crate::output::eprint_line(&render(err, &ctx, human, color));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(code: u16, reason: &str) -> GwsError {
        GwsError::Api {
            code,
            message: "boom".to_string(),
            reason: reason.to_string(),
            enable_url: None,
        }
    }

    #[test]
    fn retryable_classification() {
        assert!(api(429, "rateLimitExceeded").is_retryable());
        assert!(api(503, "backendError").is_retryable());
        assert!(api(403, "userRateLimitExceeded").is_retryable());
        assert!(api(400, "RATE_LIMIT_EXCEEDED").is_retryable());
        assert!(!api(403, "insufficientPermissions").is_retryable());
        assert!(!api(404, "notFound").is_retryable());
        assert!(!GwsError::Validation("x".into()).is_retryable());
    }

    #[test]
    fn exit_codes_distinguish_retryable_api_errors() {
        assert_eq!(CliError::from(api(500, "backendError")).exit_code(), 6);
        assert_eq!(CliError::from(api(429, "")).exit_code(), 6);
        assert_eq!(
            CliError::from(api(404, "notFound")).exit_code(),
            GwsError::EXIT_CODE_API
        );
        let codes: std::collections::HashSet<i32> =
            EXIT_CODE_DOCUMENTATION.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes.len(), EXIT_CODE_DOCUMENTATION.len());
        assert!(codes.contains(&GwsError::EXIT_CODE_API_RETRYABLE));
        assert_eq!(
            CliError::from(GwsError::ConfirmationRequired("x".into())).exit_code(),
            GwsError::EXIT_CODE_CONFIRMATION_REQUIRED
        );
    }

    #[test]
    fn clap_errors_are_validation() {
        let e = clap::Command::new("t")
            .try_get_matches_from(["t", "--nope"])
            .unwrap_err();
        let err = CliError::from(e);
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_VALIDATION);
        let env = error_envelope(&err, &ErrorContext::default());
        let msg = env["error"]["message"].as_str().unwrap();
        assert!(!msg.starts_with("error:"), "{msg}");
        assert!(msg.contains("--nope"), "{msg}");

        let e = clap::Command::new("t")
            .arg(clap::Arg::new("x").long("x").required(true))
            .try_get_matches_from(["t"])
            .unwrap_err();
        let env = error_envelope(&CliError::from(e), &ErrorContext::default());
        assert_eq!(
            env["error"]["message"],
            "the following required arguments were not provided: --x <x>"
        );
    }

    #[test]
    fn no_double_error_prefix() {
        let err = CliError::from(GwsError::Validation("error: unexpected argument".into()));
        let text = human_report(&err, &ErrorContext::default(), false);
        assert_eq!(text, "error[validation]: unexpected argument");
    }

    #[test]
    fn envelope_is_single_line_with_retryable_and_hint() {
        let err = CliError::from(api(429, "rateLimitExceeded"));
        let line = serde_json::to_string(&error_envelope(&err, &ErrorContext::default())).unwrap();
        assert!(!line.contains('\n'));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["retryable"], true);
        assert!(v["error"]["hint"].as_str().unwrap().contains("backoff"));
    }

    #[test]
    fn scope_hint_lists_required_granted_and_fix() {
        let ctx = ErrorContext {
            service: Some("gmail".into()),
            required_scopes: vec![
                "https://www.googleapis.com/auth/gmail.modify".into(),
                "https://mail.google.com/".into(),
            ],
            granted_scopes: Some(vec![
                "https://www.googleapis.com/auth/gmail.readonly".into(),
            ]),
        };
        let hint = hint_for(&api(403, "insufficientPermissions"), &ctx).unwrap();
        assert!(
            hint.contains("gmail.modify, https://mail.google.com/"),
            "{hint}"
        );
        assert!(
            hint.contains("Granted: https://www.googleapis.com/auth/gmail.readonly"),
            "{hint}"
        );
        assert!(
            hint.contains("`gwsr auth login --scopes gmail.modify`"),
            "{hint}"
        );
        // Detail reason form and unknown granted scopes.
        let ctx = ErrorContext {
            service: Some("drive".into()),
            ..Default::default()
        };
        let hint = hint_for(&api(403, "ACCESS_TOKEN_SCOPE_INSUFFICIENT"), &ctx).unwrap();
        assert!(hint.contains("gwsr auth status"), "{hint}");
        assert!(hint.contains("`gwsr auth login -s drive`"), "{hint}");
    }

    #[test]
    fn hint_table() {
        let ctx = ErrorContext::default();
        let grant = GwsError::Auth("token refresh failed".into());
        let hint = hint_for(&grant, &ctx).unwrap();
        assert!(hint.contains("gwsr auth status"), "{hint}");
        assert!(hint.contains("`gwsr auth login -s <service>`"), "{hint}");
        let scoped = ErrorContext {
            required_scopes: vec!["https://www.googleapis.com/auth/drive.readonly".into()],
            ..Default::default()
        };
        let hint = hint_for(&grant, &scoped).unwrap();
        assert!(
            hint.contains("`gwsr auth login --scopes drive.readonly`"),
            "{hint}"
        );
        let pre = hint_for(&api(400, "failedPrecondition"), &ctx).unwrap();
        assert!(pre.contains("not in a state"), "{pre}");
        let disabled = GwsError::Api {
            code: 403,
            message: "Gmail API has not been used".into(),
            reason: "accessNotConfigured".into(),
            enable_url: Some("https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=1".into()),
        };
        assert!(
            hint_for(&disabled, &ctx)
                .unwrap()
                .contains("gmail.googleapis.com/overview?project=1")
        );
        assert!(hint_for(&api(404, "notFound"), &ctx).is_none());
        assert!(hint_for(&GwsError::Validation("x".into()), &ctx).is_none());
    }

    #[test]
    fn render_defaults_to_one_json_line() {
        let err = CliError::from(api(403, "insufficientPermissions"));
        let line = render(&err, &ErrorContext::default(), false, false);
        assert!(!line.contains('\n'));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["code"], 403);
        assert!(v["error"]["hint"].as_str().unwrap().contains("gwsr auth"));
        let human = render(&err, &ErrorContext::default(), true, false);
        assert!(human.starts_with("error[api]:"), "{human}");
    }

    #[test]
    fn human_report_includes_reason_hint_and_strips_control_chars() {
        let err = CliError::from(GwsError::Api {
            code: 403,
            message: "bad\u{1b}[2J".into(),
            reason: "rateLimitExceeded".into(),
            enable_url: None,
        });
        let text = human_report(&err, &ErrorContext::default(), false);
        assert!(
            text.starts_with("error[api]: bad[2J (HTTP 403, reason: rateLimitExceeded)"),
            "{text}"
        );
        assert!(text.contains("\nhint: Rate limited"), "{text}");
        assert!(!text.contains('\u{1b}'));
    }
}
