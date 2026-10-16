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
        "API retryable  — rate limit (429/403 rateLimitExceeded), timeout (408) or transient server error (500/502/503/504); retry with backoff",
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
    (
        GwsError::EXIT_CODE_SANITIZATION_BLOCKED,
        "Blocked        — --sanitize in block mode: Model Armor matched the output, which was withheld",
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
    /// OAuth scopes a helper requested credentials for (all are needed),
    /// recorded by [`note_helper_scopes`]. Takes precedence over
    /// `required_scopes` and `service` in hints.
    pub helper_scopes: Vec<String>,
    /// Scopes the active credentials were granted, when known.
    pub granted_scopes: Option<Vec<String>>,
    /// The `gwsr auth login` command for the active profile (names it with
    /// `--profile` unless it is the implicit default). `None`: plain
    /// `gwsr auth login`.
    pub login_command: Option<String>,
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

/// Record that a helper is acquiring credentials for `scopes`, so a scope or
/// auth failure names a login that grants them. `-s <service>` would not:
/// without a terminal it grants read-only scopes, which cannot fix a write
/// helper, and `workflow` has no scopes of its own. Scopes from several
/// acquisitions (e.g. Pub/Sub plus Gmail) accumulate: the failing call needs
/// one of the sets, and a login granting all of them fixes it.
pub fn note_helper_scopes(scopes: &[&str]) {
    let mut guard = CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let ctx = guard.get_or_insert_with(ErrorContext::default);
    for scope in scopes {
        if !ctx.helper_scopes.iter().any(|s| s == scope) {
            ctx.helper_scopes.push((*scope).to_string());
        }
    }
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
             raise the request timeout for slow links (GWSR_TIMEOUT or `timeout_secs` in \
             config.toml; API methods also take --timeout). Before re-running a create or other \
             non-idempotent call, check whether it was applied. Exit code 10 marks network \
             failures."
                .to_string(),
        ),
        GwsError::SanitizationBlocked(_) => Some(
            "The request succeeded, but Model Armor matched its output, so nothing was printed \
             (sanitize mode `block`). Do not retry to get around it. To see the output with a \
             `_sanitization` annotation instead, use sanitize mode `warn` \
             (GWSR_SANITIZE_MODE=warn or `sanitize_mode` in config.toml). Exit code 11 marks \
             blocked output."
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

/// Whether a 403 means the caller may not use the `x-goog-user-project`
/// project. Drive reports this with reason `forbidden`, so the message is
/// checked too.
fn quota_project_denied(code: u16, message: &str, reason: &str) -> bool {
    reason == "USER_PROJECT_DENIED"
        || (code == 403 && message.contains("serviceusage.services.use"))
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
    if quota_project_denied(code, message, reason) {
        return Some(
            "Google refused the quota project sent as x-goog-user-project: you lack \
             serviceusage.services.use on it (common when the OAuth client belongs to a \
             project you are not a member of). Set GWSR_NO_QUOTA_PROJECT=1 (API methods \
             also take --no-quota-project) to send no quota project, set GWSR_PROJECT_ID to a \
             project you can use, or ask the project owner for \
             roles/serviceusage.serviceUsageConsumer."
                .to_string(),
        );
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
            "Rate limited by Google. Retry after an exponential backoff; for bulk work, run \
             fewer calls in parallel, or raise --page-delay (or GWSR_PAGE_DELAY_MS) when \
             paginating. Exit code 6 marks retryable errors."
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
        // The token endpoint (or something in front of it) answered without
        // an OAuth error: its status, even a 401, says nothing about the
        // credentials, so the 401 "sign in again" hint below does not apply.
        "tokenEndpointError" => Some(
            "The OAuth token endpoint's answer was no verdict on your credentials, so \
             signing in again will not help. A proxy or captive portal between gwsr and \
             Google often answers this way: check the network path (HTTPS_PROXY, a Wi-Fi \
             sign-in page) before running the command again."
                .to_string(),
        ),
        _ if code == 429 => Some(
            "Rate limited by Google. Retry after an exponential backoff. Exit code 6 \
             marks retryable errors."
                .to_string(),
        ),
        _ if code == 401 => Some(format!(
            "The access token was rejected. Sign in again with `{}`, or check GWSR_TOKEN if \
             you set it.",
            login_base(ctx)
        )),
        _ if gws_rust_core::error::is_retryable_status_code(code) => Some(
            "Google returned a transient error (timeout or server error): retry after a \
             backoff (exit code 6 marks retryable errors)."
                .to_string(),
        ),
        _ => None,
    }
}

fn scope_hint(ctx: &ErrorContext) -> String {
    let mut hint = String::from("Your credentials lack an OAuth scope this method needs.");
    if !ctx.helper_scopes.is_empty() {
        hint.push_str(" Required (all of): ");
        hint.push_str(&ctx.helper_scopes.join(", "));
        hint.push('.');
    } else if ctx.required_scopes.is_empty() {
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

/// The `gwsr auth login` command for the active profile.
fn login_base(ctx: &ErrorContext) -> &str {
    ctx.login_command.as_deref().unwrap_or("gwsr auth login")
}

/// The `gwsr auth login` command that grants what the failed call needed,
/// for the profile the failed command used.
fn login_fix(ctx: &ErrorContext) -> String {
    let login = login_base(ctx);
    if !ctx.helper_scopes.is_empty() {
        return crate::auth::scopes::login_command_hint(login, &missing_helper_scopes(ctx));
    }
    match (
        crate::auth::scopes::login_hint_scope(&ctx.required_scopes),
        ctx.service.as_deref(),
    ) {
        (Some(scope), _) => {
            crate::auth::scopes::login_command_hint(login, std::slice::from_ref(scope))
        }
        (None, Some(service)) => format!("{login} -s {service}"),
        (None, None) => format!("{login} -s <service>"),
    }
}

/// The helper scopes the recorded grant lacks; all of them when the grant is
/// unknown or (stale) claims to cover every one.
fn missing_helper_scopes(ctx: &ErrorContext) -> Vec<String> {
    use crate::auth::scopes::normalize_scope;
    let Some(granted) = &ctx.granted_scopes else {
        return ctx.helper_scopes.clone();
    };
    let granted: Vec<String> = granted.iter().map(|s| normalize_scope(s)).collect();
    let missing: Vec<String> = ctx
        .helper_scopes
        .iter()
        .filter(|s| !granted.contains(&normalize_scope(s)))
        .cloned()
        .collect();
    if missing.is_empty() {
        ctx.helper_scopes.clone()
    } else {
        missing
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
            GwsError::SanitizationBlocked(_) => "blocked",
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
            let mut envelope = json!({
                "error": {
                    "code": 400,
                    "message": strip_error_prefix(&summary.join(" ")),
                    "reason": "validationError",
                    "retryable": false,
                }
            });
            // clap's suggestions ("a similar argument exists: '--params'")
            // are the hint; the human form prints them too.
            let tips: Vec<&str> = rendered
                .lines()
                .filter_map(|l| l.trim().strip_prefix("tip:"))
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect();
            if !tips.is_empty()
                && let Some(obj) = envelope.get_mut("error").and_then(Value::as_object_mut)
            {
                obj.insert("hint".into(), Value::String(tips.join("; ")));
            }
            envelope
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
            // clap already prints `error: …` plus usage and tips. It quotes
            // argument values verbatim, and arguments are often written by an
            // LLM, so the text is sanitized like any other untrusted text.
            // That rules out clap's own colors: user-supplied escape sequences
            // could not be told apart from them.
            sanitize_for_terminal(e.render().to_string().trim_end())
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
    // Messages carry API- and filter-derived text (`--jq` `error(...)`).
    // serde_json escapes C0 controls but not C1 controls (U+009B is an 8-bit
    // CSI to some terminals) or bidi overrides, so they are escaped here.
    let line = match serde_json::to_string(&error_envelope(err, ctx)) {
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
    };
    crate::output::escape_json_for_terminal(line)
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
    if ctx.login_command.is_none() {
        // Best effort, like the granted scopes: without it the hint names
        // plain `gwsr auth login`.
        match crate::auth::active_login_command() {
            Ok(login) => ctx.login_command = Some(login),
            Err(e) => tracing::debug!(error = %format!("{e:#}"), "active profile unavailable"),
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

    /// clap's "did you mean" tips, which the human form prints, are the
    /// envelope's `hint`; an error without tips has none.
    #[test]
    fn clap_suggestions_become_the_hint() {
        let cmd = clap::Command::new("t")
            .arg(clap::Arg::new("params").long("params"))
            .subcommand(clap::Command::new("files"));
        let e = cmd
            .clone()
            .try_get_matches_from(["t", "--param", "x"])
            .unwrap_err();
        let env = error_envelope(&CliError::from(e), &ErrorContext::default());
        assert_eq!(
            env["error"]["message"],
            "unexpected argument '--param' found"
        );
        assert_eq!(
            env["error"]["hint"],
            "a similar argument exists: '--params'"
        );

        let e = cmd.try_get_matches_from(["t", "fils"]).unwrap_err();
        let env = error_envelope(&CliError::from(e), &ErrorContext::default());
        assert_eq!(env["error"]["hint"], "a similar subcommand exists: 'files'");

        let e = clap::Command::new("t")
            .arg(clap::Arg::new("x").long("x").required(true))
            .try_get_matches_from(["t"])
            .unwrap_err();
        let env = error_envelope(&CliError::from(e), &ErrorContext::default());
        assert!(env["error"].get("hint").is_none(), "{env}");
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
            login_command: None,
            helper_scopes: Vec::new(),
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

    /// Chat methods list app-only scopes (chat.bot, chat.import) first;
    /// Google refuses those in a user sign-in, so the fix names the first
    /// scope a user can grant.
    #[test]
    fn login_fix_skips_app_only_scopes() {
        let ctx = ErrorContext {
            service: Some("chat".into()),
            required_scopes: vec![
                "https://www.googleapis.com/auth/chat.bot".into(),
                "https://www.googleapis.com/auth/chat.import".into(),
                "https://www.googleapis.com/auth/chat.messages".into(),
            ],
            ..Default::default()
        };
        for err in [
            GwsError::Auth("x".into()),
            api(403, "insufficientPermissions"),
        ] {
            let hint = hint_for(&err, &ctx).unwrap();
            assert!(
                hint.contains("`gwsr auth login --scopes chat.messages`"),
                "{hint}"
            );
            assert!(!hint.contains("--scopes chat.bot"), "{hint}");
        }
    }

    /// The fix in a hint signs in to the profile the failed command used.
    #[test]
    fn hints_name_the_active_profile() {
        let ctx = ErrorContext {
            service: Some("drive".into()),
            required_scopes: vec!["https://www.googleapis.com/auth/drive.readonly".into()],
            login_command: Some("gwsr auth login --profile work".into()),
            ..Default::default()
        };
        let scope = hint_for(&api(403, "insufficientPermissions"), &ctx).unwrap();
        assert!(
            scope.contains("`gwsr auth login --profile work --scopes drive.readonly`"),
            "{scope}"
        );
        let auth = hint_for(&GwsError::Auth("x".into()), &ctx).unwrap();
        assert!(
            auth.contains("`gwsr auth login --profile work --scopes drive.readonly`"),
            "{auth}"
        );
        let ctx = ErrorContext {
            service: Some("drive".into()),
            login_command: Some("gwsr auth login --profile work".into()),
            ..Default::default()
        };
        let auth = hint_for(&GwsError::Auth("x".into()), &ctx).unwrap();
        assert!(
            auth.contains("`gwsr auth login --profile work -s drive`"),
            "{auth}"
        );
        let rejected = hint_for(&api(401, "authError"), &ctx).unwrap();
        assert!(
            rejected.contains("`gwsr auth login --profile work`"),
            "{rejected}"
        );
    }

    /// A helper's scope or auth failure names a login for the scopes the
    /// helper requested (all of them, minus any already granted), for the
    /// active profile; never `-s <service>`, which grants read-only scopes
    /// without a terminal.
    #[test]
    fn helper_hints_name_the_requested_scopes() {
        let ctx = ErrorContext {
            service: Some("workflow".into()),
            helper_scopes: vec![
                "https://www.googleapis.com/auth/drive.readonly".into(),
                "https://www.googleapis.com/auth/chat.messages.create".into(),
            ],
            login_command: Some("gwsr auth login --profile work".into()),
            ..Default::default()
        };
        let fix = "`gwsr auth login --profile work --scopes drive.readonly,chat.messages.create`";
        let scope = hint_for(&api(403, "insufficientPermissions"), &ctx).unwrap();
        assert!(scope.contains("Required (all of): "), "{scope}");
        assert!(scope.contains(fix), "{scope}");
        let auth = hint_for(&GwsError::Auth("x".into()), &ctx).unwrap();
        assert!(auth.contains(fix), "{auth}");
        assert!(!auth.contains(" -s "), "{auth}");

        let granted = ErrorContext {
            granted_scopes: Some(vec!["drive.readonly".into(), "openid".into()]),
            ..ctx.clone()
        };
        let scope = hint_for(&api(403, "insufficientPermissions"), &granted).unwrap();
        assert!(
            scope.contains("`gwsr auth login --profile work --scopes chat.messages.create`"),
            "{scope}"
        );
        // A grant that (stale) claims every scope still names them all.
        let stale = ErrorContext {
            granted_scopes: Some(ctx.helper_scopes.clone()),
            ..ctx.clone()
        };
        let scope = hint_for(&api(403, "insufficientPermissions"), &stale).unwrap();
        assert!(scope.contains(fix), "{scope}");
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

    /// A token-endpoint 401 without an OAuth error (a proxy's answer) says
    /// the credentials were not rejected; its hint must not tell the user to
    /// sign in again, as the hint for an API's 401 does.
    #[test]
    fn token_endpoint_failure_hint_does_not_say_sign_in_again() {
        let ctx = ErrorContext::default();
        for status in [401, 403, 407, 501] {
            let err = GwsError::from(crate::auth::AuthError::TokenEndpointFailed {
                status,
                detail: "<html>Proxy</html>".into(),
            });
            assert!(
                err.to_string().contains("credentials were not rejected"),
                "{err}"
            );
            let hint = hint_for(&err, &ctx).unwrap_or_default();
            assert!(
                hint.contains("no verdict on your credentials"),
                "{status}: {hint}"
            );
            assert!(!hint.contains("was rejected"), "{status}: {hint}");
        }
        // An API's own 401 still says to sign in again.
        let api_401 = hint_for(&api(401, "authError"), &ctx).unwrap();
        assert!(api_401.contains("Sign in again"), "{api_401}");
    }

    /// `--timeout`, `--no-quota-project` and `--page-delay` exist only on
    /// generated API methods (and `--page-delay` only on paginated ones), so a
    /// hint that names one also names a setting that works for every command
    /// (helpers, `gwsr batch`), and never a setting gwsr does not have.
    #[test]
    fn hints_name_settings_that_every_command_honors() {
        let ctx = ErrorContext::default();
        let network = hint_for(&GwsError::Network("timed out".into()), &ctx).unwrap();
        assert!(network.contains("GWSR_TIMEOUT"), "{network}");
        assert!(network.contains("timeout_secs"), "{network}");
        let quota = hint_for(&api(403, "USER_PROJECT_DENIED"), &ctx).unwrap();
        assert!(quota.contains("Set GWSR_NO_QUOTA_PROJECT=1"), "{quota}");
        assert!(!quota.contains("Pass --no-quota-project"), "{quota}");
        let rate = hint_for(&api(429, "rateLimitExceeded"), &ctx).unwrap();
        assert!(rate.contains("GWSR_PAGE_DELAY_MS"), "{rate}");
        assert!(!rate.contains("concurrency"), "{rate}");
    }

    #[test]
    fn quota_project_denied_points_at_no_quota_project() {
        // Upstream googleworkspace/cli#729: the caller is not a member of the
        // project sent as x-goog-user-project. Drive reports reason "forbidden"
        // (errors[0]); other APIs report USER_PROJECT_DENIED.
        let ctx = ErrorContext::default();
        let denied = GwsError::Api {
            code: 403,
            message: "Caller does not have required permission to use project my-proj. \
                      Grant the caller the roles/serviceusage.serviceUsageConsumer role, \
                      or a custom role with the serviceusage.services.use permission"
                .into(),
            reason: "forbidden".into(),
            enable_url: None,
        };
        let hint = hint_for(&denied, &ctx).expect("a hint for USER_PROJECT_DENIED");
        assert!(hint.contains("x-goog-user-project"), "{hint}");
        assert!(hint.contains("--no-quota-project"), "{hint}");
        assert!(hint.contains("GWSR_NO_QUOTA_PROJECT=1"), "{hint}");
        assert!(hint.contains("GWSR_PROJECT_ID"), "{hint}");
        let by_reason = hint_for(&api(403, "USER_PROJECT_DENIED"), &ctx).unwrap();
        assert!(by_reason.contains("--no-quota-project"), "{by_reason}");
        // An ordinary permission error is not misattributed to the quota project.
        assert!(hint_for(&api(403, "forbidden"), &ctx).is_none());
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

    /// An API message or a `--jq` `error(...)` value can hold C1 controls
    /// (U+009B is an 8-bit CSI) and bidi overrides, which serde_json does not
    /// escape: neither form of the error lets them reach the terminal raw.
    #[test]
    fn error_output_never_carries_c1_or_bidi_controls() {
        let message = "a\u{9b}31mb\u{202E}c\u{2066}d\u{85}e\u{1b}[2Jf";
        let err = CliError::from(GwsError::Validation(message.into()));
        let line = render(&err, &ErrorContext::default(), false, false);
        let raw = |c: char| c.is_control() || crate::output::is_dangerous_unicode(c);
        assert!(!line.chars().any(raw), "{line:?}");
        assert!(
            line.contains("\\u009b") && line.contains("\\u202e"),
            "{line}"
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["message"], message);
        for color in [false, true] {
            let human = render(&err, &ErrorContext::default(), true, color);
            assert!(!human.chars().any(|c| c != '\u{1b}' && raw(c)), "{human:?}");
            assert!(human.contains("a31mbcde[2Jf"), "{human:?}");
        }
    }

    /// clap quotes argument values verbatim; the human form strips the
    /// controls from them as well.
    #[test]
    fn human_clap_errors_are_sanitized() {
        let parsed = clap::Command::new("gwsr")
            .arg(
                clap::Arg::new("format")
                    .long("format")
                    .value_parser(["json", "table"]),
            )
            .try_get_matches_from(["gwsr", "--format", "x\u{9b}31m\u{1b}[2J\u{202E}y"]);
        let err = CliError::Clap(parsed.unwrap_err());
        for color in [false, true] {
            let human = render(&err, &ErrorContext::default(), true, color);
            assert!(
                !human.chars().any(
                    |c| (c.is_control() && c != '\n') || crate::output::is_dangerous_unicode(c)
                ),
                "{human:?}"
            );
            assert!(human.contains("invalid value"), "{human}");
        }
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
