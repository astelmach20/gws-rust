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

//! Structured error types for Google Workspace API operations.

use serde_json::json;
use thiserror::Error;

/// Error reasons Google uses for per-user and per-project rate limits.
pub(crate) const RATE_LIMIT_REASONS: &[&str] = &[
    "rateLimitExceeded",
    "userRateLimitExceeded",
    "RATE_LIMIT_EXCEEDED",
];

/// Boxed error type carried by [`GwsError::Other`].
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Top-level error type for Google Workspace operations.
///
/// Each variant maps to a stable process exit code via [`GwsError::exit_code`]
/// and to a machine-readable JSON envelope via [`GwsError::to_json`].
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum GwsError {
    /// The Google API returned an error response.
    #[error("{message}")]
    Api {
        code: u16,
        message: String,
        reason: String,
        /// For `accessNotConfigured` errors: the GCP console URL to enable the API.
        enable_url: Option<String>,
    },

    /// Invalid user input (arguments, paths, identifiers).
    #[error("{0}")]
    Validation(String),

    /// Authentication or authorization failure.
    #[error("{0}")]
    Auth(String),

    /// Discovery Document could not be fetched, parsed, or trusted.
    #[error("{0}")]
    Discovery(String),

    /// A destructive or gated action was not confirmed: no `--yes` and no
    /// terminal to prompt on, or the user declined the prompt. Nothing was
    /// sent.
    #[error("{0}")]
    ConfirmationRequired(String),

    /// Any other unexpected failure. The source chain is preserved.
    #[error("{}", format_chain(.0.as_ref()))]
    Other(BoxError),
}

/// Render an error and its `source()` chain as `outer: inner: root`.
fn format_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = err.to_string();
    let mut current = err.source();
    while let Some(src) = current {
        let text = src.to_string();
        // anyhow-originated errors already render their chain in Display;
        // avoid repeating a cause that is already part of the message.
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        current = src.source();
    }
    out
}

impl GwsError {
    /// Exit code for non-retryable [`GwsError::Api`] errors.
    pub const EXIT_CODE_API: i32 = 1;
    /// Exit code for [`GwsError::Auth`] variants.
    pub const EXIT_CODE_AUTH: i32 = 2;
    /// Exit code for [`GwsError::Validation`] variants.
    pub const EXIT_CODE_VALIDATION: i32 = 3;
    /// Exit code for [`GwsError::Discovery`] variants.
    pub const EXIT_CODE_DISCOVERY: i32 = 4;
    /// Exit code for [`GwsError::Other`] variants.
    pub const EXIT_CODE_OTHER: i32 = 5;
    /// Exit code for retryable [`GwsError::Api`] errors (HTTP 429, 5xx, or
    /// 403 rate-limit reasons). See [`GwsError::is_retryable`].
    pub const EXIT_CODE_API_RETRYABLE: i32 = 6;
    /// Exit code for [`GwsError::ConfirmationRequired`].
    pub const EXIT_CODE_CONFIRMATION_REQUIRED: i32 = 7;

    /// Wrap any error (or message) as [`GwsError::Other`].
    pub fn other(err: impl Into<BoxError>) -> Self {
        GwsError::Other(err.into())
    }

    /// Whether retrying the same request later may succeed.
    ///
    /// True for API errors with HTTP 429, any 5xx, or a rate-limit reason
    /// (`rateLimitExceeded`, `userRateLimitExceeded`, `RATE_LIMIT_EXCEEDED`).
    pub fn is_retryable(&self) -> bool {
        match self {
            GwsError::Api { code, reason, .. } => {
                *code == 429 || *code >= 500 || RATE_LIMIT_REASONS.contains(&reason.as_str())
            }
            _ => false,
        }
    }

    /// Map each error variant to a stable, documented exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            GwsError::Api { .. } if self.is_retryable() => Self::EXIT_CODE_API_RETRYABLE,
            GwsError::Api { .. } => Self::EXIT_CODE_API,
            GwsError::Auth(_) => Self::EXIT_CODE_AUTH,
            GwsError::Validation(_) => Self::EXIT_CODE_VALIDATION,
            GwsError::Discovery(_) => Self::EXIT_CODE_DISCOVERY,
            GwsError::ConfirmationRequired(_) => Self::EXIT_CODE_CONFIRMATION_REQUIRED,
            GwsError::Other(_) => Self::EXIT_CODE_OTHER,
        }
    }

    /// Machine-readable JSON envelope: `{"error": {"code", "message", "reason", ...}}`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            } => {
                let mut error_obj = json!({
                    "code": code,
                    "message": message,
                    "reason": reason,
                });
                if let Some(url) = enable_url {
                    error_obj["enable_url"] = json!(url);
                }
                json!({ "error": error_obj })
            }
            GwsError::Validation(msg) => json!({
                "error": {
                    "code": 400,
                    "message": msg,
                    "reason": "validationError",
                }
            }),
            GwsError::Auth(msg) => json!({
                "error": {
                    "code": 401,
                    "message": msg,
                    "reason": "authError",
                }
            }),
            GwsError::Discovery(msg) => json!({
                "error": {
                    "code": 500,
                    "message": msg,
                    "reason": "discoveryError",
                }
            }),
            GwsError::ConfirmationRequired(msg) => json!({
                "error": {
                    "code": 412,
                    "message": msg,
                    "reason": "confirmationRequired",
                }
            }),
            GwsError::Other(_) => json!({
                "error": {
                    "code": 500,
                    "message": self.to_string(),
                    "reason": "internalError",
                }
            }),
        }
    }
}

#[cfg(feature = "anyhow")]
impl From<anyhow::Error> for GwsError {
    fn from(err: anyhow::Error) -> Self {
        // `{:#}` renders the full anyhow context chain in one line.
        GwsError::Other(format!("{err:#}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_api() {
        let err = GwsError::Api {
            code: 404,
            message: "Not Found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
    }

    #[test]
    fn test_exit_code_auth() {
        assert_eq!(
            GwsError::Auth("bad token".to_string()).exit_code(),
            GwsError::EXIT_CODE_AUTH
        );
    }

    #[test]
    fn test_exit_code_validation() {
        assert_eq!(
            GwsError::Validation("missing arg".to_string()).exit_code(),
            GwsError::EXIT_CODE_VALIDATION
        );
    }

    #[test]
    fn test_exit_code_discovery() {
        assert_eq!(
            GwsError::Discovery("fetch failed".to_string()).exit_code(),
            GwsError::EXIT_CODE_DISCOVERY
        );
    }

    #[test]
    fn test_exit_code_other() {
        assert_eq!(
            GwsError::other("oops").exit_code(),
            GwsError::EXIT_CODE_OTHER
        );
    }

    #[test]
    fn test_confirmation_required_has_its_own_exit_code_and_reason() {
        let err = GwsError::ConfirmationRequired("pass --yes".to_string());
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_CONFIRMATION_REQUIRED);
        assert_eq!(err.to_json()["error"]["reason"], "confirmationRequired");
        assert_eq!(err.to_json()["error"]["message"], "pass --yes");
    }

    #[test]
    fn test_exit_codes_are_distinct() {
        let codes = [
            GwsError::EXIT_CODE_API,
            GwsError::EXIT_CODE_AUTH,
            GwsError::EXIT_CODE_VALIDATION,
            GwsError::EXIT_CODE_DISCOVERY,
            GwsError::EXIT_CODE_OTHER,
            GwsError::EXIT_CODE_API_RETRYABLE,
            GwsError::EXIT_CODE_CONFIRMATION_REQUIRED,
        ];
        let unique: std::collections::HashSet<i32> = codes.iter().copied().collect();
        assert_eq!(
            unique.len(),
            codes.len(),
            "exit codes must be distinct: {codes:?}"
        );
    }

    #[test]
    fn test_error_to_json_api() {
        let err = GwsError::Api {
            code: 404,
            message: "Not Found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 404);
        assert_eq!(json["error"]["message"], "Not Found");
        assert_eq!(json["error"]["reason"], "notFound");
        assert!(json["error"]["enable_url"].is_null());
    }

    #[test]
    fn test_error_to_json_validation() {
        let err = GwsError::Validation("Invalid input".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 400);
        assert_eq!(json["error"]["message"], "Invalid input");
        assert_eq!(json["error"]["reason"], "validationError");
    }

    #[test]
    fn test_error_to_json_auth() {
        let err = GwsError::Auth("Token expired".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 401);
        assert_eq!(json["error"]["message"], "Token expired");
        assert_eq!(json["error"]["reason"], "authError");
    }

    #[test]
    fn test_error_to_json_discovery() {
        let err = GwsError::Discovery("Failed to fetch doc".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Failed to fetch doc");
        assert_eq!(json["error"]["reason"], "discoveryError");
    }

    #[test]
    fn test_error_to_json_other() {
        let err = GwsError::other("Something went wrong");
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Something went wrong");
        assert_eq!(json["error"]["reason"], "internalError");
    }

    #[test]
    fn test_error_to_json_access_not_configured_with_url() {
        let err = GwsError::Api {
            code: 403,
            message: "Gmail API has not been used in project 549352339482 before or it is disabled.".to_string(),
            reason: "accessNotConfigured".to_string(),
            enable_url: Some("https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482".to_string()),
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 403);
        assert_eq!(json["error"]["reason"], "accessNotConfigured");
        assert_eq!(
            json["error"]["enable_url"],
            "https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482"
        );
    }

    #[test]
    fn test_error_to_json_access_not_configured_without_url() {
        let err = GwsError::Api {
            code: 403,
            message: "API not enabled.".to_string(),
            reason: "accessNotConfigured".to_string(),
            enable_url: None,
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 403);
        assert_eq!(json["error"]["reason"], "accessNotConfigured");
        assert!(json["error"]["enable_url"].is_null());
    }

    fn api(code: u16, reason: &str) -> GwsError {
        GwsError::Api {
            code,
            message: "m".to_string(),
            reason: reason.to_string(),
            enable_url: None,
        }
    }

    #[test]
    fn retryable_api_errors_get_distinct_exit_code() {
        for err in [
            api(429, "rateLimitExceeded"),
            api(500, "backendError"),
            api(503, "unavailable"),
            api(403, "rateLimitExceeded"),
            api(403, "userRateLimitExceeded"),
        ] {
            assert!(err.is_retryable(), "{err:?}");
            assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API_RETRYABLE);
        }
    }

    #[test]
    fn permanent_api_errors_are_not_retryable() {
        for err in [
            api(400, "badRequest"),
            api(403, "forbidden"),
            api(404, "notFound"),
        ] {
            assert!(!err.is_retryable(), "{err:?}");
            assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
        }
        assert!(!GwsError::Validation("x".into()).is_retryable());
    }

    #[derive(Debug, thiserror::Error)]
    #[error("outer failure")]
    struct Outer(#[source] std::io::Error);

    #[test]
    fn other_renders_source_chain() {
        let err = GwsError::other(Outer(std::io::Error::other("disk full")));
        assert_eq!(err.to_string(), "outer failure: disk full");
        assert_eq!(
            err.to_json()["error"]["message"],
            "outer failure: disk full"
        );
    }

    #[cfg(feature = "anyhow")]
    #[test]
    fn anyhow_conversion_keeps_context_chain() {
        use anyhow::Context;
        let res: Result<(), std::io::Error> = Err(std::io::Error::other("root cause"));
        let err: GwsError = res.context("while loading").unwrap_err().into();
        assert_eq!(err.to_string(), "while loading: root cause");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_OTHER);
    }
}
