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

//! Minimal authenticated REST client shared by the Gmail, Events, Workflow,
//! and Model Armor helpers.
//!
//! Two properties matter here:
//!
//! * **Errors are never swallowed.** Non-2xx responses become a
//!   [`GwsError::Api`] that carries the real HTTP status and Google's error
//!   message/reason. An unreadable error body is reported as such, never
//!   replaced with a placeholder that hides the failure.
//! * **Non-idempotent requests are never retried.** [`Retry::Never`] sends the
//!   request exactly once, so a timeout on `messages.send`, a Chat message, or a
//!   Tasks insert cannot produce a duplicate. Only [`Retry::Idempotent`]
//!   requests go through the shared retry policy.

use crate::error::GwsError;
use reqwest::Method;
use serde_json::Value;

/// Whether a request may be retried on transient failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Retry {
    /// Safe to repeat (GET, idempotent PUT/DELETE, set-semantics modify).
    Idempotent,
    /// Must be sent at most once (message send, create calls).
    Never,
}

/// Build a `GwsError` for an unexpected condition (not an API response).
pub(crate) fn other_error(msg: impl std::fmt::Display) -> GwsError {
    anyhow::anyhow!("{msg}").into()
}

/// Build a `GwsError::Api` from an HTTP error response body, parsing the
/// Google JSON error format when possible.
pub(crate) fn api_error(status: u16, body: &str, context: &str) -> GwsError {
    let err_json: Option<Value> = serde_json::from_str(body).ok();
    let err_obj = err_json.as_ref().and_then(|v| v.get("error"));
    let message = err_obj
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("HTTP {status} with empty response body")
            } else {
                body.to_string()
            }
        });
    let reason = err_obj
        .and_then(|e| e.get("errors"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|e| e.get("reason"))
        .and_then(Value::as_str)
        .or_else(|| {
            err_obj
                .and_then(|e| e.get("status"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            err_obj
                .and_then(|e| e.get("reason"))
                .and_then(Value::as_str)
        })
        .unwrap_or("httpError")
        .to_string();
    let enable_url = if reason == "accessNotConfigured" {
        crate::executor::extract_enable_url(&message)
    } else {
        None
    };
    GwsError::Api {
        code: status,
        message: format!("{context}: {message}"),
        reason,
        enable_url,
    }
}

/// Convert a non-success response into a `GwsError::Api`, reading the body.
///
/// If the body itself cannot be read, the error says so explicitly while
/// still carrying the real status code.
pub(crate) async fn error_from_response(resp: reqwest::Response, context: &str) -> GwsError {
    let status = resp.status().as_u16();
    match resp.text().await {
        Ok(body) => api_error(status, &body, context),
        Err(e) => GwsError::Api {
            code: status,
            message: format!("{context}: HTTP {status} (error body unreadable: {e})"),
            reason: "httpError".to_string(),
            enable_url: None,
        },
    }
}

/// Read a successful response as JSON. Empty bodies (e.g. 204) become `{}`.
pub(crate) async fn json_body(resp: reqwest::Response, context: &str) -> Result<Value, GwsError> {
    let text = resp
        .text()
        .await
        .map_err(|e| other_error(format!("{context}: failed to read response body: {e}")))?;
    if text.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(&text)
        .map_err(|e| other_error(format!("{context}: response is not valid JSON: {e}")))
}

/// Check the status of a response and parse its JSON body.
pub(crate) async fn expect_json(resp: reqwest::Response, context: &str) -> Result<Value, GwsError> {
    if !resp.status().is_success() {
        return Err(error_from_response(resp, context).await);
    }
    json_body(resp, context).await
}

/// An authenticated JSON REST client.
#[derive(Clone)]
pub(crate) struct RestClient {
    http: reqwest::Client,
    token: String,
}

impl RestClient {
    pub(crate) fn new(http: reqwest::Client, token: impl Into<String>) -> Self {
        Self {
            http,
            token: token.into(),
        }
    }

    /// The bearer token (for APIs called through other helpers).
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// Send a request built by `build` (which receives a fresh, authenticated
    /// `RequestBuilder` for each attempt) and return the raw response.
    pub(crate) async fn send(
        &self,
        method: Method,
        url: &str,
        retry: Retry,
        context: &str,
        build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, GwsError> {
        let make = || {
            build(
                self.http
                    .request(method.clone(), url)
                    .bearer_auth(&self.token),
            )
        };
        let idempotency = match retry {
            Retry::Idempotent => crate::client::Idempotency::Idempotent,
            Retry::Never => crate::client::Idempotency::NonIdempotent,
        };
        let policy = crate::client::RetryPolicy::from_env()?;
        let result = crate::client::send(&policy, idempotency, make)
            .await
            .map(|sent| sent.response);
        result.map_err(|e| other_error(format!("{context}: request failed: {e}")))
    }

    /// Send a JSON request and parse a JSON response, failing on non-2xx.
    pub(crate) async fn json(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        retry: Retry,
        context: &str,
    ) -> Result<Value, GwsError> {
        let resp = self
            .send(method, url, retry, context, |rb| {
                let rb = rb.query(query);
                match body {
                    Some(b) => rb.json(b),
                    None => rb,
                }
            })
            .await?;
        expect_json(resp, context).await
    }

    /// Convenience: idempotent GET returning JSON.
    pub(crate) async fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        context: &str,
    ) -> Result<Value, GwsError> {
        self.json(Method::GET, url, query, None, Retry::Idempotent, context)
            .await
    }
}

/// Resolve the output format from the global `--format` flag, failing loudly
/// on unknown values.
pub(crate) fn output_format(
    matches: &clap::ArgMatches,
    default: crate::formatter::OutputFormat,
) -> Result<crate::formatter::OutputFormat, GwsError> {
    match matches.try_get_one::<String>("format") {
        Ok(Some(s)) => crate::formatter::OutputFormat::parse(s).map_err(|unknown| {
            GwsError::Validation(format!(
                "Unknown --format '{unknown}' (valid: json, table, yaml, csv)"
            ))
        }),
        Ok(None) => Ok(default),
        Err(e) => Err(other_error(format!("Failed to read --format: {e}"))),
    }
}

/// Whether the global `--dry-run` flag is set.
pub(crate) fn dry_run(matches: &clap::ArgMatches) -> Result<bool, GwsError> {
    matches
        .try_get_one::<bool>("dry-run")
        .map(|v| v.copied().unwrap_or(false))
        .map_err(|e| other_error(format!("Failed to read --dry-run: {e}")))
}

/// A request description printed by `--dry-run`.
pub(crate) fn dry_run_request(
    method: &str,
    url: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Value {
    let query: serde_json::Map<String, Value> = query
        .iter()
        .map(|(k, v)| ((*k).to_string(), Value::String(v.clone())))
        .collect();
    serde_json::json!({
        "method": method,
        "url": url,
        "query_params": query,
        "body": body.cloned().unwrap_or(Value::Null),
    })
}

/// Print a dry-run plan (a list of requests) as JSON on stdout.
pub(crate) fn print_dry_run(requests: Vec<Value>) -> Result<(), GwsError> {
    let plan = serde_json::json!({ "dry_run": true, "requests": requests });
    let text = serde_json::to_string_pretty(&plan)
        .map_err(|e| other_error(format!("Failed to serialize dry-run plan: {e}")))?;
    println!("{text}");
    Ok(())
}

/// SEC-25 confirmation gate for destructive and outbound helper actions.
///
/// Mirrors `helpers/confirm.rs` from the helpers-apps workstream so both
/// helper families behave identically:
///
/// * [`Impact::Destructive`] is always gated.
/// * [`Impact::Outbound`] is gated only when `GWSR_REQUIRE_CONFIRM` is truthy
///   (`1`/`true`; `0`/`false`/empty mean off; anything else is an error).
/// * A gated action proceeds with `--yes` or `--dry-run`; otherwise, on an
///   interactive terminal the user is prompted, and without one it fails.
pub(crate) mod confirm {
    use crate::error::GwsError;
    use clap::{Arg, ArgAction, ArgMatches, Command};
    use std::io::{BufRead, IsTerminal, Write};

    pub(crate) const REQUIRE_CONFIRM_ENV: &str = "GWSR_REQUIRE_CONFIRM";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Impact {
        /// Irreversible changes (e.g. deleting a filter). Always gated.
        Destructive,
        /// Actions visible to other people (sending mail). Gated by policy.
        Outbound,
    }

    /// Add the `--yes`/`-y` flag to a command.
    pub(crate) fn with_yes(cmd: Command) -> Command {
        cmd.arg(
            Arg::new("yes")
                .long("yes")
                .short('y')
                .help("Skip the confirmation prompt for this action")
                .action(ArgAction::SetTrue),
        )
    }

    /// The error returned when confirmation is required but not given.
    pub(crate) fn confirmation_required(action: &str) -> GwsError {
        GwsError::Validation(format!(
            "Confirmation required: {action}. Re-run with --yes to proceed (or --dry-run to preview)."
        ))
    }

    /// Parse the policy value of `GWSR_REQUIRE_CONFIRM`.
    pub(crate) fn parse_policy(value: Option<std::ffi::OsString>) -> Result<bool, GwsError> {
        let Some(value) = value else {
            return Ok(false);
        };
        let Some(v) = value.to_str() else {
            return Err(GwsError::Validation(format!(
                "{REQUIRE_CONFIRM_ENV} is not valid UTF-8"
            )));
        };
        match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" => Ok(true),
            "0" | "false" | "" => Ok(false),
            other => Err(GwsError::Validation(format!(
                "Invalid {REQUIRE_CONFIRM_ENV} value '{other}' (expected 1/true or 0/false)"
            ))),
        }
    }

    /// Whether an action of this impact needs confirmation under the current policy.
    pub(crate) fn is_gated(impact: Impact) -> Result<bool, GwsError> {
        match impact {
            Impact::Destructive => Ok(true),
            Impact::Outbound => parse_policy(std::env::var_os(REQUIRE_CONFIRM_ENV)),
        }
    }

    fn flag(matches: &ArgMatches, name: &str) -> Result<bool, GwsError> {
        matches
            .try_get_one::<bool>(name)
            .map(|v| v.copied().unwrap_or(false))
            .map_err(|e| GwsError::Validation(format!("Failed to read --{name}: {e}")))
    }

    /// Decide whether `action` may proceed. Returns `Ok(())` to proceed.
    pub(crate) fn confirm(
        matches: &ArgMatches,
        impact: Impact,
        action: &str,
    ) -> Result<(), GwsError> {
        if !is_gated(impact)? || flag(matches, "yes")? || flag(matches, "dry-run")? {
            return Ok(());
        }
        let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        if !interactive {
            return Err(confirmation_required(action));
        }
        let mut stderr = std::io::stderr().lock();
        write!(stderr, "About to {action}. Proceed? [y/N] ")
            .and_then(|()| stderr.flush())
            .map_err(|e| GwsError::Validation(format!("Failed to prompt for confirmation: {e}")))?;
        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| GwsError::Validation(format!("Failed to read confirmation: {e}")))?;
        decide(&answer, action)
    }

    /// Interpret an interactive answer.
    pub(crate) fn decide(answer: &str, action: &str) -> Result<(), GwsError> {
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => Ok(()),
            _ => Err(GwsError::Validation(format!("Aborted: did not {action}"))),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn matches(args: &[&str]) -> ArgMatches {
            with_yes(Command::new("t"))
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue),
                )
                .try_get_matches_from(args)
                .unwrap()
        }

        #[test]
        fn policy_parsing() {
            assert!(!parse_policy(None).unwrap());
            assert!(parse_policy(Some("1".into())).unwrap());
            assert!(parse_policy(Some("TRUE".into())).unwrap());
            assert!(!parse_policy(Some("0".into())).unwrap());
            assert!(!parse_policy(Some("".into())).unwrap());
            assert!(parse_policy(Some("yes".into())).is_err());
        }

        #[test]
        fn destructive_requires_yes_without_tty() {
            // Test runners have no TTY on stdin, so this cannot prompt.
            if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
                return;
            }
            let err =
                confirm(&matches(&["t"]), Impact::Destructive, "delete filter f1").unwrap_err();
            assert!(err.to_string().contains("--yes"));
            assert!(confirm(&matches(&["t", "--yes"]), Impact::Destructive, "x").is_ok());
            assert!(confirm(&matches(&["t", "-y"]), Impact::Destructive, "x").is_ok());
            assert!(confirm(&matches(&["t", "--dry-run"]), Impact::Destructive, "x").is_ok());
        }

        #[test]
        fn decide_accepts_only_yes() {
            assert!(decide("y\n", "x").is_ok());
            assert!(decide("YES", "x").is_ok());
            assert!(decide("", "x").is_err());
            assert!(
                decide("n", "x")
                    .unwrap_err()
                    .to_string()
                    .contains("Aborted")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn api_error_parses_google_json_format() {
        let body = r#"{"error":{"code":403,"message":"Insufficient Permission","errors":[{"reason":"insufficientPermissions"}]}}"#;
        match api_error(403, body, "Ctx") {
            GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            } => {
                assert_eq!(code, 403);
                assert_eq!(message, "Ctx: Insufficient Permission");
                assert_eq!(reason, "insufficientPermissions");
                assert!(enable_url.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_falls_back_to_raw_body() {
        match api_error(502, "Bad Gateway", "Ctx") {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(code, 502);
                assert_eq!(message, "Ctx: Bad Gateway");
                assert_eq!(reason, "httpError");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_uses_status_field_and_mentions_empty_body() {
        let body = r#"{"error":{"code":404,"message":"nope","status":"NOT_FOUND"}}"#;
        match api_error(404, body, "Ctx") {
            GwsError::Api { reason, .. } => assert_eq!(reason, "NOT_FOUND"),
            other => panic!("unexpected {other:?}"),
        }
        match api_error(500, "", "Ctx") {
            GwsError::Api { message, .. } => assert!(message.contains("empty response body")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_extracts_top_level_reason() {
        let body = r#"{"error":{"message":"x","reason":"topLevel"}}"#;
        match api_error(400, body, "Ctx") {
            GwsError::Api { reason, .. } => assert_eq!(reason, "topLevel"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_access_not_configured_extracts_url() {
        let body = r#"{"error":{"code":403,"message":"Gmail API has not been used in project 1 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=1 then retry.","errors":[{"reason":"accessNotConfigured"}]}}"#;
        match api_error(403, body, "Ctx") {
            GwsError::Api {
                reason, enable_url, ..
            } => {
                assert_eq!(reason, "accessNotConfigured");
                assert!(enable_url.is_some());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn json_propagates_real_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/thing"))
            .respond_with(ResponseTemplate::new(418).set_body_string("teapot"))
            .mount(&server)
            .await;
        let client = RestClient::new(reqwest::Client::new(), "tok");
        let err = client
            .get(&format!("{}/thing", server.uri()), &[], "Get thing")
            .await
            .unwrap_err();
        match err {
            GwsError::Api { code, message, .. } => {
                assert_eq!(code, 418);
                assert!(message.contains("teapot"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn never_retry_sends_exactly_once_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/send"))
            .respond_with(ResponseTemplate::new(429))
            .expect(1)
            .mount(&server)
            .await;
        let client = RestClient::new(reqwest::Client::new(), "tok");
        let result = client
            .json(
                Method::POST,
                &format!("{}/send", server.uri()),
                &[],
                Some(&serde_json::json!({})),
                Retry::Never,
                "Send",
            )
            .await;
        assert!(result.is_err());
        // `expect(1)` is verified when the server drops.
    }

    #[tokio::test]
    async fn never_retry_does_not_resend_on_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/send"))
            .respond_with(
                ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(500)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .unwrap();
        let client = RestClient::new(http, "tok");
        let err = client
            .json(
                Method::POST,
                &format!("{}/send", server.uri()),
                &[],
                None,
                Retry::Never,
                "Send",
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("request failed"));
    }

    #[tokio::test]
    async fn empty_success_body_is_empty_object() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = RestClient::new(reqwest::Client::new(), "tok");
        let v = client
            .json(
                Method::DELETE,
                &format!("{}/x", server.uri()),
                &[],
                None,
                Retry::Idempotent,
                "Delete",
            )
            .await
            .unwrap();
        assert_eq!(v, serde_json::json!({}));
    }

    #[tokio::test]
    async fn invalid_json_success_body_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        let client = RestClient::new(reqwest::Client::new(), "tok");
        let err = client
            .get(&format!("{}/x", server.uri()), &[], "Get")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn dry_run_request_shape() {
        let v = dry_run_request(
            "POST",
            "https://x/y",
            &[("a", "b".to_string())],
            Some(&serde_json::json!({"k": 1})),
        );
        assert_eq!(v["method"], "POST");
        assert_eq!(v["query_params"]["a"], "b");
        assert_eq!(v["body"]["k"], 1);
    }
}
