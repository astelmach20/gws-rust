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

//! Mapping of HTTP error responses to [`GwsError`].

use serde_json::Value;

use super::AuthMethod;
use crate::error::GwsError;

/// Longest non-JSON error body quoted in an error message.
const MAX_RAW_ERROR_CHARS: usize = 2000;

/// Attempts to extract a GCP console enable URL from a Google API
/// `accessNotConfigured` error message.
///
/// The message format is typically:
/// `"<API> has not been used in project <N> before or it is disabled. Enable it by visiting <URL> then retry."`
pub fn extract_enable_url(message: &str) -> Option<String> {
    let after_visiting = message.split("visiting ").nth(1)?;
    let url = after_visiting
        .split_whitespace()
        .next()
        .map(|s| {
            s.trim_end_matches(|c: char| ['.', ',', ';', ':', ')', ']', '"', '\''].contains(&c))
        })
        .filter(|s| s.starts_with("http"))?;
    Some(url.to_string())
}

/// Prefix `context` (e.g. "Failed to send message") to an API, network or
/// internal error. Validation, auth, configuration and discovery errors
/// already say what failed and are returned unchanged.
pub(crate) fn with_context(err: GwsError, context: &str) -> GwsError {
    match err {
        GwsError::Api {
            code,
            message,
            reason,
            enable_url,
        } => GwsError::Api {
            code,
            message: format!("{context}: {message}"),
            reason,
            enable_url,
        },
        GwsError::Other(source) => GwsError::Other(Box::new(Context {
            context: context.to_string(),
            source,
        })),
        GwsError::Network(source) => GwsError::Network(Box::new(Context {
            context: context.to_string(),
            source,
        })),
        other => other,
    }
}

/// An internal error with a caller-supplied prefix. The original error stays
/// reachable through `source()` (e.g. for [`super::is_timeout`]).
#[derive(Debug)]
struct Context {
    context: String,
    source: gws_rust_core::error::BoxError,
}

impl std::fmt::Display for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.context)
    }
}

impl std::error::Error for Context {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Convert an error response into a [`GwsError`].
///
/// `retry_note` (from [`gws_rust_core::client::Sent::retry_note`]) is appended
/// to the message so users can see that the request was already retried.
pub(crate) fn error_from_response(
    status: reqwest::StatusCode,
    error_body: &str,
    auth_method: &AuthMethod,
    retry_note: Option<&str>,
) -> GwsError {
    let with_note = |message: String| match retry_note {
        Some(note) => format!("{message} ({note})"),
        None => message,
    };

    if matches!(status.as_u16(), 401 | 403) && *auth_method == AuthMethod::None {
        return GwsError::Auth(
            "Access denied. No credentials provided. Run `gwsr auth login` or set \
             GWSR_CREDENTIALS_FILE to an OAuth credentials JSON file."
                .to_string(),
        );
    }

    if let Ok(error_json) = serde_json::from_str::<Value>(error_body)
        && let Some(err_obj) = error_json.get("error")
        && err_obj.is_object()
    {
        let code = err_obj
            .get("code")
            .and_then(Value::as_u64)
            .and_then(|c| u16::try_from(c).ok())
            .unwrap_or(status.as_u16());
        let message = err_obj
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {status} with no error message"));
        // Reason can appear in "errors[0].reason" or at the top-level "reason" field.
        let reason = err_obj
            .get("errors")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|e| e.get("reason"))
            .and_then(Value::as_str)
            .or_else(|| err_obj.get("reason").and_then(Value::as_str))
            .or_else(|| err_obj.get("status").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_string();
        let enable_url = if reason == "accessNotConfigured" || reason == "SERVICE_DISABLED" {
            extract_enable_url(&message)
        } else {
            None
        };
        return GwsError::Api {
            code,
            message: with_note(message),
            reason,
            enable_url,
        };
    }

    let trimmed = error_body.trim();
    let message = if trimmed.is_empty() {
        format!("HTTP {status} with an empty response body")
    } else {
        // Non-JSON bodies (HTML error pages) are capped so they cannot flood
        // the error line.
        trimmed.chars().take(MAX_RAW_ERROR_CHARS).collect()
    };
    GwsError::Api {
        code: status.as_u16(),
        message: with_note(message),
        reason: "httpError".to_string(),
        enable_url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api_parts(err: GwsError) -> (u16, String, String, Option<String>) {
        match err {
            GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            } => (code, message, reason, enable_url),
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[test]
    fn unauthenticated_401_gives_login_hint() {
        let err = error_from_response(
            reqwest::StatusCode::UNAUTHORIZED,
            "Unauthorized",
            &AuthMethod::None,
            None,
        );
        assert!(matches!(err, GwsError::Auth(ref m) if m.contains("Access denied")));
    }

    #[test]
    fn oauth_401_keeps_server_message() {
        let body = json!({"error": {"code": 401, "message": "Request had invalid authentication credentials.",
                                    "errors": [{"reason": "authError"}]}})
        .to_string();
        let (code, message, reason, _) = api_parts(error_from_response(
            reqwest::StatusCode::UNAUTHORIZED,
            &body,
            &AuthMethod::OAuth,
            None,
        ));
        assert_eq!(code, 401);
        assert!(message.contains("invalid authentication credentials"));
        assert_eq!(reason, "authError");
    }

    #[test]
    fn api_error_with_retry_note() {
        let body = json!({"error": {"code": 503, "message": "Backend Error", "errors": [{"reason": "backendError"}]}}).to_string();
        let (code, message, reason, _) = api_parts(error_from_response(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            &body,
            &AuthMethod::OAuth,
            Some("gave up after 4 attempts"),
        ));
        assert_eq!(code, 503);
        assert_eq!(message, "Backend Error (gave up after 4 attempts)");
        assert_eq!(reason, "backendError");
    }

    #[test]
    fn non_json_body_is_http_error() {
        let (code, message, reason, _) = api_parts(error_from_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error Text",
            &AuthMethod::OAuth,
            None,
        ));
        assert_eq!(code, 500);
        assert_eq!(message, "Internal Server Error Text");
        assert_eq!(reason, "httpError");
    }

    #[test]
    fn empty_body_is_described() {
        let (_, message, _, _) = api_parts(error_from_response(
            reqwest::StatusCode::BAD_GATEWAY,
            "",
            &AuthMethod::OAuth,
            None,
        ));
        assert!(message.contains("empty response body"));
    }

    #[test]
    fn access_not_configured_top_level_reason() {
        let body = json!({"error": {"code": 403,
            "message": "Gmail API has not been used in project 549352339482 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482 then retry.",
            "status": "PERMISSION_DENIED", "reason": "accessNotConfigured"}})
        .to_string();
        let (_, _, reason, url) = api_parts(error_from_response(
            reqwest::StatusCode::FORBIDDEN,
            &body,
            &AuthMethod::OAuth,
            None,
        ));
        assert_eq!(reason, "accessNotConfigured");
        assert_eq!(
            url.as_deref(),
            Some(
                "https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482"
            )
        );
    }

    #[test]
    fn access_not_configured_errors_array() {
        let body = json!({"error": {"code": 403,
            "message": "Drive API has not been used in project 12345 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/drive.googleapis.com/overview?project=12345 then retry.",
            "errors": [{"reason": "accessNotConfigured"}]}})
        .to_string();
        let (_, _, reason, url) = api_parts(error_from_response(
            reqwest::StatusCode::FORBIDDEN,
            &body,
            &AuthMethod::OAuth,
            None,
        ));
        assert_eq!(reason, "accessNotConfigured");
        assert!(url.unwrap().contains("drive.googleapis.com"));
    }

    #[test]
    fn extract_enable_url_cases() {
        assert_eq!(extract_enable_url("API not enabled."), None);
        assert_eq!(
            extract_enable_url("Enable it by visiting ftp://example.com then retry."),
            None
        );
        assert_eq!(
            extract_enable_url(
                "Enable it by visiting https://console.cloud.google.com/apis/library?project=test123. Then retry."
            )
            .as_deref(),
            Some("https://console.cloud.google.com/apis/library?project=test123")
        );
    }

    fn oauth(status: u16, body: &str) -> (u16, String, String, Option<String>) {
        api_parts(error_from_response(
            reqwest::StatusCode::from_u16(status).unwrap(),
            body,
            &AuthMethod::OAuth,
            None,
        ))
    }

    #[test]
    fn non_json_and_empty_bodies_are_reported() {
        assert!(oauth(502, "").1.contains("empty response body"));
        assert_eq!(oauth(500, "<html>boom</html>").1, "<html>boom</html>");
        assert_eq!(oauth(500, &"x".repeat(5000)).1.len(), MAX_RAW_ERROR_CHARS);
        assert_eq!(oauth(502, "Bad Gateway").2, "httpError");
    }

    #[test]
    fn v2_status_field_is_the_reason() {
        let body = r#"{"error":{"code":403,"message":"denied","status":"PERMISSION_DENIED"}}"#;
        assert_eq!(oauth(403, body).2, "PERMISSION_DENIED");
    }

    #[test]
    fn service_disabled_carries_enable_url() {
        let body = r#"{"error":{"code":403,"message":"Enable it by visiting https://console.developers.google.com/apis/api/x then retry.","status":"SERVICE_DISABLED"}}"#;
        assert_eq!(
            oauth(403, body).3.as_deref(),
            Some("https://console.developers.google.com/apis/api/x")
        );
    }

    #[test]
    fn with_context_prefixes_api_and_internal_errors_only() {
        let err = with_context(
            error_from_response(
                reqwest::StatusCode::NOT_FOUND,
                "nope",
                &AuthMethod::OAuth,
                None,
            ),
            "Failed to get x",
        );
        assert_eq!(err.to_string(), "Failed to get x: nope");
        assert_eq!(
            with_context(GwsError::other("boom"), "ctx").to_string(),
            "ctx: boom"
        );
        let v = with_context(GwsError::Validation("bad".into()), "ctx");
        assert!(matches!(v, GwsError::Validation(ref m) if m == "bad"));
    }
}
