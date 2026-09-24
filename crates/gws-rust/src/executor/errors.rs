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
        let enable_url = if reason == "accessNotConfigured" {
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

    let message = if error_body.trim().is_empty() {
        format!("HTTP {status} with an empty response body")
    } else {
        error_body.to_string()
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
}
