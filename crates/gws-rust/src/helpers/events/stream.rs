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

//! Long-running Pub/Sub pull loops shared by `gmail +watch` and
//! `events +subscribe` (BP P1-4, UX #680).
//!
//! * **Transient failures do not kill the stream.** 408/429/5xx responses,
//!   network errors and token-refresh failures are retried with capped
//!   exponential backoff and jitter. `--max-failures N` tolerates N
//!   consecutive failures: the loop exits with an error on failure N+1, or
//!   immediately on a permanent
//!   failure (other 4xx, invalid configuration, local I/O errors).
//! * **Errors carry the real HTTP status.**
//! * **Diagnostics are single-line JSON on stderr**, so stdout stays a clean
//!   NDJSON data stream.

use crate::error::GwsError;
use crate::transport::Transport;
use gws_rust_core::client::{Idempotency, RetryPolicy};
use serde_json::{Value, json};
use std::time::Duration;

/// Classification of one failed loop iteration.
#[derive(Debug)]
pub(crate) enum StepError {
    /// Retry after a backoff (counts against the failure budget). The error
    /// is kept whole: when the budget runs out it is the command's error,
    /// with its own class (a lost connection exits 10, a refused token 2, a
    /// 403 rate limit 6).
    Transient(GwsError),
    /// Stop the loop and report this error.
    Fatal(GwsError),
}

impl From<GwsError> for StepError {
    /// API errors are transient exactly when they are retryable (exit code 6:
    /// a retryable status or a 403 rate-limit reason); validation and
    /// discovery errors are permanent; anything else (network failures,
    /// token refresh failures, unreadable bodies) is treated as transient and
    /// bounded by the failure budget.
    fn from(e: GwsError) -> Self {
        match &e {
            GwsError::Api { .. } if e.is_retryable() => StepError::Transient(e),
            GwsError::Api { .. } | GwsError::Validation(_) | GwsError::Discovery(_) => {
                StepError::Fatal(e)
            }
            _ => StepError::Transient(e),
        }
    }
}

/// Prints a stream's records to stdout through the shared formatter, so
/// `--format`, `--jq` and `--columns` apply to every record, as they do to a
/// generated method's `--page-all` stream (JSON records are compact NDJSON;
/// CSV and table print one header).
pub(crate) struct StreamPrinter {
    format: crate::formatter::OutputFormat,
    pages: crate::formatter::PageStream,
}

impl StreamPrinter {
    pub(crate) fn new(format: crate::formatter::OutputFormat) -> Self {
        Self {
            format,
            pages: crate::formatter::PageStream::default(),
        }
    }

    /// Print one record. A write or formatting failure stops the stream.
    pub(crate) fn print(&mut self, record: &Value) -> Result<(), StepError> {
        crate::formatter::format_value_paginated(record, &self.format, &mut self.pages)
            .and_then(|text| crate::output::emit(&text))
            .map_err(StepError::Fatal)
    }
}

/// Write one diagnostic record to stderr as a single JSON line. With `-q`
/// only `error` records are written.
pub(crate) fn emit_diagnostic(level: &str, event: &str, fields: Value) {
    if level != "error" && crate::logging::quiet() {
        return;
    }
    crate::output::eprint_line(&diagnostic_line(level, event, fields));
}

/// The JSON line [`emit_diagnostic`] writes. Fields carry API- and
/// message-derived text, so C1 controls and bidi overrides, which serde_json
/// writes raw, are escaped as in the error envelope.
fn diagnostic_line(level: &str, event: &str, fields: Value) -> String {
    let mut record = json!({ "level": level, "event": event });
    if let (Some(obj), Value::Object(extra)) = (record.as_object_mut(), fields) {
        obj.extend(extra);
    }
    crate::output::escape_json_for_terminal(record.to_string())
}

/// Consecutive-failure budget with capped, jittered exponential backoff.
#[derive(Debug, Clone)]
pub(crate) struct FailureBudget {
    max_consecutive: u32,
    consecutive: u32,
    base: Duration,
    cap: Duration,
}

impl FailureBudget {
    pub(crate) fn new(max_consecutive: u32, base: Duration, cap: Duration) -> Self {
        Self {
            max_consecutive,
            consecutive: 0,
            base,
            cap,
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.consecutive = 0;
    }

    /// Record a failure. Returns the delay before retrying, or `None` when
    /// the budget is exhausted.
    pub(crate) fn record_failure(&mut self) -> Option<Duration> {
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive > self.max_consecutive {
            return None;
        }
        let exp = self.consecutive.saturating_sub(1).min(16);
        let full = self.base.saturating_mul(1u32 << exp).min(self.cap);
        // Equal jitter: half fixed, half random, to avoid synchronized retries.
        let half = full / 2;
        // Saturating; `half` is bounded by `cap`, so this never clamps.
        let jitter_ms = u64::try_from(half.as_millis()).unwrap_or(u64::MAX);
        let random = if jitter_ms == 0 {
            0
        } else {
            rand::random::<u64>() % (jitter_ms + 1)
        };
        Some(half + Duration::from_millis(random))
    }

    pub(crate) fn consecutive(&self) -> u32 {
        self.consecutive
    }
}

/// Loop settings.
#[derive(Debug, Clone)]
pub(crate) struct StreamOptions {
    pub once: bool,
    pub poll_interval: Duration,
    pub max_failures: u32,
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
}

impl StreamOptions {
    pub(crate) fn new(once: bool, poll_interval_secs: u64, max_failures: u32) -> Self {
        Self {
            once,
            poll_interval: Duration::from_secs(poll_interval_secs),
            max_failures,
            backoff_base: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(60),
        }
    }
}

/// One iteration of a pull loop: a pull, then the handling of what it
/// received.
///
/// A shutdown signal cancels only [`StreamStep::pull`]. Once messages are
/// received, [`StreamStep::handle`] runs to the end: it may print some of
/// them and must then acknowledge those, or Pub/Sub redelivers what was
/// already printed. To stop promptly, `handle` checks
/// [`crate::helpers::shutdown_signal`] itself between messages (see
/// [`interruptible`]).
pub(crate) trait StreamStep {
    async fn pull(&mut self) -> Result<Vec<ReceivedMessage>, StepError>;
    async fn handle(&mut self, received: Vec<ReceivedMessage>) -> Result<(), StepError>;
}

/// Run `work` unless a shutdown signal arrives first (or arrived already):
/// `None` means it was cancelled by the signal.
pub(crate) async fn interruptible<T>(work: impl std::future::Future<Output = T>) -> Option<T> {
    tokio::select! {
        biased;
        _ = crate::helpers::shutdown_signal() => None,
        r = work => Some(r),
    }
}

/// Sleep for `d`, returning `false` if a shutdown signal arrived first.
async fn sleep_or_shutdown(d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => true,
        _ = crate::helpers::shutdown_signal() => {
            emit_diagnostic("info", "shutdown", json!({}));
            false
        }
    }
}

/// Run `step` until shutdown, `--once` completion, a fatal error, or an
/// exhausted failure budget.
pub(crate) async fn run_stream<S: StreamStep>(
    opts: &StreamOptions,
    step: &mut S,
) -> Result<(), GwsError> {
    let mut budget = FailureBudget::new(opts.max_failures, opts.backoff_base, opts.backoff_cap);
    loop {
        // A signal latched before the stream started (during setup) stops
        // it before the first pull. Only the pull is cancelled: messages it
        // received are handled (and what was printed acknowledged).
        let Some(pulled) = interruptible(step.pull()).await else {
            emit_diagnostic("info", "shutdown", json!({}));
            return Ok(());
        };
        let result = match pulled {
            Ok(received) => step.handle(received).await,
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                budget.record_success();
                if crate::helpers::shutdown_requested() {
                    emit_diagnostic("info", "shutdown", json!({}));
                    return Ok(());
                }
                if opts.once {
                    return Ok(());
                }
                if !sleep_or_shutdown(opts.poll_interval).await {
                    return Ok(());
                }
            }
            Err(StepError::Fatal(e)) => return Err(e),
            Err(StepError::Transient(e)) => {
                let Some(delay) = budget.record_failure() else {
                    // `consecutive()` already counts this final failure:
                    // `--max-failures N` tolerates N and stops on the N+1th.
                    let context = format!(
                        "giving up after {} consecutive transient failures \
                         (--max-failures {}); last error",
                        budget.consecutive(),
                        opts.max_failures
                    );
                    return Err(crate::helpers::modelarmor::prefix_message(e, &context));
                };
                let status = match &e {
                    GwsError::Api { code, .. } => Some(*code),
                    _ => None,
                };
                let message = e.to_string();
                emit_diagnostic(
                    "warning",
                    "transient_error",
                    json!({
                        "status": status,
                        "message": message,
                        "attempt": budget.consecutive(),
                        "maxFailures": opts.max_failures,
                        // Saturating conversion for display only.
                        "retryInMs": u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    }),
                );
                if !sleep_or_shutdown(delay).await {
                    return Ok(());
                }
            }
        }
    }
}

/// A message received from a Pub/Sub pull.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReceivedMessage {
    pub ack_id: String,
    /// The Pub/Sub `message` object (data, attributes, messageId, ...).
    pub message: Value,
}

/// Parse a `:pull` response.
pub(crate) fn parse_pull_response(resp: &Value) -> Result<Vec<ReceivedMessage>, GwsError> {
    let Some(received) = resp.get("receivedMessages") else {
        return Ok(Vec::new());
    };
    let received = received.as_array().ok_or_else(|| {
        GwsError::other("Pub/Sub pull response: receivedMessages is not an array")
    })?;
    received
        .iter()
        .map(|m| {
            let ack_id = m
                .get("ackId")
                .and_then(Value::as_str)
                .ok_or_else(|| GwsError::other("Pub/Sub pull response: message without ackId"))?;
            Ok(ReceivedMessage {
                ack_id: ack_id.to_string(),
                message: m.get("message").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// Minimal Pub/Sub REST client on the shared transport.
///
/// Every call is a single attempt: the pull loop's [`FailureBudget`] owns
/// retries, so core retries would only multiply the backoff. The transport
/// still refreshes the token once on a 401.
#[derive(Clone)]
pub(crate) struct PubSubClient {
    transport: Transport,
    base: String,
}

impl PubSubClient {
    pub(crate) fn new(transport: &Transport) -> Self {
        Self::with_base(
            transport,
            &crate::helpers::pubsub_api_base(transport.endpoints()),
        )
    }

    pub(crate) fn with_base(transport: &Transport, base: &str) -> Self {
        let single = transport.with_retry(RetryPolicy {
            max_attempts: 1,
            ..transport.retry.clone()
        });
        Self {
            transport: single,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.base)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        url: String,
        body: Option<&Value>,
        context: &str,
    ) -> Result<Value, GwsError> {
        // The error keeps its class: one-shot setup and cleanup calls exit
        // with it (a 403 rate limit is 6, a network failure 10, a refused
        // token 2); only stream steps map it to a `StepError`.
        self.transport
            .json(method, &url, &[], body, Idempotency::FromMethod, context)
            .await
    }

    /// Create a topic (PUT).
    pub(crate) async fn create_topic(&self, topic: &str) -> Result<(), GwsError> {
        self.call(
            reqwest::Method::PUT,
            self.url(topic),
            Some(&json!({})),
            "Failed to create Pub/Sub topic",
        )
        .await
        .map(|_| ())
    }

    /// Create a pull subscription on `topic`.
    pub(crate) async fn create_subscription(&self, sub: &str, topic: &str) -> Result<(), GwsError> {
        let body = json!({ "topic": topic, "ackDeadlineSeconds": 60 });
        self.call(
            reqwest::Method::PUT,
            self.url(sub),
            Some(&body),
            "Failed to create Pub/Sub subscription",
        )
        .await
        .map(|_| ())
    }

    /// Grant `role` on `topic` to `member` (replaces the topic policy).
    pub(crate) async fn set_topic_publisher(
        &self,
        topic: &str,
        member: &str,
    ) -> Result<(), GwsError> {
        let body = json!({
            "policy": { "bindings": [{ "role": "roles/pubsub.publisher", "members": [member] }] }
        });
        self.call(
            reqwest::Method::POST,
            format!("{}:setIamPolicy", self.url(topic)),
            Some(&body),
            "Failed to grant publish permission on the Pub/Sub topic",
        )
        .await
        .map(|_| ())
    }

    /// Pull up to `max` messages. No response within `timeout` means "no
    /// messages yet".
    pub(crate) async fn pull(
        &self,
        sub: &str,
        max: u32,
        timeout: Duration,
    ) -> Result<Vec<ReceivedMessage>, StepError> {
        let body = json!({ "maxMessages": max });
        let url = format!("{}:pull", self.url(sub));
        let policy = RetryPolicy {
            max_attempts: 1,
            response_timeout: Some(timeout),
            ..self.transport.retry.clone()
        };
        let value = match self
            .transport
            .with_retry(policy)
            .json(
                reqwest::Method::POST,
                &url,
                &[],
                Some(&body),
                // Pulling is safe to repeat: unacknowledged messages are redelivered.
                Idempotency::Idempotent,
                "Pub/Sub pull failed",
            )
            .await
        {
            Ok(v) => v,
            Err(e) if crate::transport::is_timeout(&e) => return Ok(Vec::new()),
            Err(e) => return Err(StepError::from(e)),
        };
        parse_pull_response(&value).map_err(StepError::Fatal)
    }

    /// Acknowledge messages.
    pub(crate) async fn ack(&self, sub: &str, ack_ids: &[String]) -> Result<(), StepError> {
        if ack_ids.is_empty() {
            return Ok(());
        }
        self.call(
            reqwest::Method::POST,
            format!("{}:acknowledge", self.url(sub)),
            Some(&json!({ "ackIds": ack_ids })),
            "Pub/Sub acknowledge failed",
        )
        .await
        .map(|_| ())
        .map_err(StepError::from)
    }

    /// Delete a topic or subscription.
    pub(crate) async fn delete(&self, name: &str) -> Result<(), GwsError> {
        self.call(
            reqwest::Method::DELETE,
            self.url(name),
            None,
            &format!("Failed to delete {name}"),
        )
        .await
        .map(|_| ())
    }
}

/// Delete created Pub/Sub resources, reporting every failure. Returns the
/// first failure's error, with its own class (a refused permission exits 1,
/// a lost connection 10), naming any later failures too. Each error already
/// names its resource.
pub(crate) async fn cleanup_resources(
    client: &PubSubClient,
    names: &[&str],
) -> Result<(), GwsError> {
    let mut failed = Vec::new();
    for name in names {
        match client.delete(name).await {
            Ok(()) => emit_diagnostic("info", "deleted", json!({ "resource": name })),
            Err(e) => failed.push(e),
        }
    }
    let mut failed = failed.into_iter();
    let Some(first) = failed.next() else {
        return Ok(());
    };
    let later: Vec<String> = failed.map(|e| e.to_string()).collect();
    let context = if later.is_empty() {
        "Failed to delete Pub/Sub resources".to_string()
    } else {
        format!(
            "Failed to delete Pub/Sub resources (also: {})",
            later.join("; ")
        )
    };
    Err(crate::helpers::modelarmor::prefix_message(first, &context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Diagnostic fields carry API- and message-derived text (a Pub/Sub
    /// error, a Gmail message id, a Model Armor reason). serde_json leaves C1
    /// controls (U+009B is an 8-bit CSI) and bidi overrides raw, so the line
    /// escapes them, like the error envelope does.
    #[test]
    fn diagnostic_line_escapes_c1_and_bidi_controls() {
        let text = "a\u{9b}31mb\u{202E}c\u{2066}d\u{85}e\u{1b}[2Jf";
        let line = diagnostic_line("warning", "message_gone", json!({ "id": text }));
        assert!(
            !line
                .chars()
                .any(|c| c.is_control() || crate::output::is_dangerous_unicode(c)),
            "{line:?}"
        );
        assert!(
            line.contains("\\u009b") && line.contains("\\u202e"),
            "{line}"
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], text);
        assert_eq!(v["level"], "warning");
        assert_eq!(v["event"], "message_gone");
    }

    #[test]
    fn transient_classification() {
        let api = |code: u16| GwsError::Api {
            code,
            message: "x".into(),
            reason: "r".into(),
            enable_url: None,
        };
        for code in [408, 429, 500, 502, 503, 504] {
            assert!(matches!(
                StepError::from(api(code)),
                StepError::Transient(_)
            ));
        }
        for code in [400, 401, 403, 404, 409, 501] {
            assert!(matches!(StepError::from(api(code)), StepError::Fatal(_)));
        }
        let e = GwsError::Api {
            code: 503,
            message: "x".into(),
            reason: "r".into(),
            enable_url: None,
        };
        assert!(matches!(
            StepError::from(e),
            StepError::Transient(GwsError::Api { code: 503, .. })
        ));
        let e = GwsError::Api {
            code: 403,
            message: "x".into(),
            reason: "r".into(),
            enable_url: None,
        };
        assert!(matches!(StepError::from(e), StepError::Fatal(_)));
        assert!(matches!(
            StepError::from(GwsError::Validation("v".into())),
            StepError::Fatal(_)
        ));
        assert!(matches!(
            StepError::from(GwsError::other("net")),
            StepError::Transient(GwsError::Other(_))
        ));
    }

    /// A 403 rate limit exits 6 (retryable) and is retried by the client, so
    /// the stream must treat it as transient too instead of exiting on it.
    #[test]
    fn rate_limited_403_is_transient() {
        for reason in [
            "rateLimitExceeded",
            "userRateLimitExceeded",
            "RATE_LIMIT_EXCEEDED",
        ] {
            let e = GwsError::Api {
                code: 403,
                message: "x".into(),
                reason: reason.into(),
                enable_url: None,
            };
            assert!(e.is_retryable(), "{reason}");
            assert!(
                matches!(
                    StepError::from(e),
                    StepError::Transient(GwsError::Api { code: 403, .. })
                ),
                "{reason}"
            );
        }
    }

    #[test]
    fn budget_backs_off_and_exhausts() {
        let mut b = FailureBudget::new(3, Duration::from_millis(100), Duration::from_millis(300));
        let d1 = b.record_failure().unwrap();
        assert!(d1 >= Duration::from_millis(50) && d1 <= Duration::from_millis(100));
        let d2 = b.record_failure().unwrap();
        assert!(d2 >= Duration::from_millis(100) && d2 <= Duration::from_millis(200));
        let d3 = b.record_failure().unwrap();
        assert!(d3 <= Duration::from_millis(300), "capped");
        assert!(b.record_failure().is_none());
        b.record_success();
        assert!(b.record_failure().is_some());
    }

    #[test]
    fn parse_pull_response_requires_ack_ids() {
        assert!(parse_pull_response(&json!({})).unwrap().is_empty());
        let ok = parse_pull_response(
            &json!({"receivedMessages": [{"ackId": "a", "message": {"data": "e30="}}]}),
        )
        .unwrap();
        assert_eq!(ok[0].ack_id, "a");
        assert!(parse_pull_response(&json!({"receivedMessages": [{"message": {}}]})).is_err());
    }

    fn fast(once: bool, max_failures: u32) -> StreamOptions {
        StreamOptions {
            once,
            poll_interval: Duration::from_millis(1),
            max_failures,
            backoff_base: Duration::from_millis(1),
            backoff_cap: Duration::from_millis(2),
        }
    }

    struct Scripted {
        results: std::collections::VecDeque<Result<(), StepError>>,
        calls: u32,
    }

    impl StreamStep for Scripted {
        async fn pull(&mut self) -> Result<Vec<ReceivedMessage>, StepError> {
            self.calls += 1;
            self.results
                .pop_front()
                .unwrap_or(Ok(()))
                .map(|()| Vec::new())
        }

        async fn handle(&mut self, _received: Vec<ReceivedMessage>) -> Result<(), StepError> {
            Ok(())
        }
    }

    fn transient(code: u16) -> Result<(), StepError> {
        Err(StepError::Transient(GwsError::Api {
            code,
            message: format!("HTTP {code}"),
            reason: "backendError".into(),
            enable_url: None,
        }))
    }

    #[tokio::test]
    async fn stream_survives_transient_errors() {
        let mut s = Scripted {
            results: [transient(503), transient(429), Ok(())].into(),
            calls: 0,
        };
        run_stream(&fast(true, 5), &mut s).await.unwrap();
        assert_eq!(s.calls, 3);
    }

    #[tokio::test]
    async fn stream_gives_up_after_budget_with_real_status() {
        let mut s = Scripted {
            results: [transient(503), transient(503), transient(502)].into(),
            calls: 0,
        };
        let err = run_stream(&fast(true, 2), &mut s).await.unwrap_err();
        // --max-failures 2 tolerates two failures and stops on the third:
        // the message counts every failed attempt that was made.
        assert_eq!(s.calls, 3);
        match err {
            GwsError::Api { code, message, .. } => {
                assert_eq!(code, 502);
                assert!(
                    message.contains("giving up after 3 consecutive transient failures"),
                    "{message}"
                );
                assert!(message.contains("--max-failures 2"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// When the budget runs out, the last failure decides the exit code: a
    /// lost connection is a network error (10), a refused token an auth
    /// error (2), and a 403 rate limit keeps its reason so it stays
    /// retryable (6). None of them is an internal error (5).
    #[tokio::test]
    async fn exhausted_budget_keeps_the_last_error_class() {
        let rate_limited = || GwsError::Api {
            code: 403,
            message: "Quota exceeded".into(),
            reason: "rateLimitExceeded".into(),
            enable_url: None,
        };
        let cases: [(fn() -> GwsError, i32); 3] = [
            (
                || GwsError::Network("connection refused".into()),
                GwsError::EXIT_CODE_NETWORK,
            ),
            (
                || GwsError::Auth("token refresh failed".into()),
                GwsError::EXIT_CODE_AUTH,
            ),
            (rate_limited, GwsError::EXIT_CODE_API_RETRYABLE),
        ];
        for (make, exit) in cases {
            let mut s = Scripted {
                results: [Err(StepError::from(make())), Err(StepError::from(make()))].into(),
                calls: 0,
            };
            let err = run_stream(&fast(true, 1), &mut s).await.unwrap_err();
            assert_eq!(err.exit_code(), exit, "{err:?}");
            assert_eq!(s.calls, 2);
            assert!(err.to_string().contains("giving up after 2"), "{err}");
            assert!(err.to_string().contains(&make().to_string()), "{err}");
        }
    }

    #[tokio::test]
    async fn stream_stops_on_fatal_error() {
        let fatal = Err(StepError::Fatal(GwsError::Api {
            code: 404,
            message: "gone".into(),
            reason: "notFound".into(),
            enable_url: None,
        }));
        let mut s = Scripted {
            results: [fatal].into(),
            calls: 0,
        };
        let err = run_stream(&fast(false, 5), &mut s).await.unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 404, .. }));
        assert_eq!(s.calls, 1);
    }

    #[tokio::test]
    async fn success_resets_budget() {
        let mut s = Scripted {
            results: [
                transient(500),
                transient(500),
                Ok(()),
                transient(500),
                transient(500),
                Err(StepError::Fatal(GwsError::Validation("stop".into()))),
            ]
            .into(),
            calls: 0,
        };
        let err = run_stream(&fast(false, 2), &mut s).await.unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)));
        assert_eq!(s.calls, 6);
    }

    #[tokio::test]
    async fn pull_propagates_real_status_and_classifies() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/p/subscriptions/s:pull"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            // The loop's failure budget owns retries: one attempt per step.
            .expect(1)
            .mount(&server)
            .await;
        let client = PubSubClient::with_base(
            &Transport::for_test(&server.uri()),
            &format!("{}/v1", server.uri()),
        );
        let err = client
            .pull("projects/p/subscriptions/s", 10, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            StepError::Transient(GwsError::Api { code: 503, .. })
        ));
    }

    /// Setup calls run once, outside the stream's failure budget, so their
    /// errors must keep their class: a 403 rate limit still exits 6 (not 1
    /// with its reason replaced) and a network failure exits 10 (not 5).
    #[tokio::test]
    async fn setup_calls_keep_the_error_class() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/v1/projects/p/topics/t"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {
                    "code": 403,
                    "message": "Quota exceeded",
                    "errors": [{"reason": "rateLimitExceeded"}]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = PubSubClient::with_base(
            &Transport::for_test(&server.uri()),
            &format!("{}/v1", server.uri()),
        );
        let err = client
            .create_topic("projects/p/topics/t")
            .await
            .unwrap_err();
        assert_eq!(
            err.exit_code(),
            GwsError::EXIT_CODE_API_RETRYABLE,
            "{err:?}"
        );
        assert!(matches!(&err, GwsError::Api { reason, .. } if reason == "rateLimitExceeded"));

        // Nothing listens on the discard port: the connection is refused.
        let offline = PubSubClient::with_base(
            &Transport::for_test("http://127.0.0.1:9/"),
            "http://127.0.0.1:9/v1",
        );
        for err in [
            offline
                .create_subscription("projects/p/subscriptions/s", "projects/p/topics/t")
                .await
                .unwrap_err(),
            offline.delete("projects/p/topics/t").await.unwrap_err(),
        ] {
            assert_eq!(err.exit_code(), GwsError::EXIT_CODE_NETWORK, "{err:?}");
        }
    }

    #[test]
    fn pubsub_client_follows_the_api_base_override() {
        let client = PubSubClient::new(&Transport::for_test("http://127.0.0.1:9/"));
        assert_eq!(
            client.url("projects/p/topics/t"),
            "http://127.0.0.1:9/v1/projects/p/topics/t"
        );
        assert_eq!(
            crate::helpers::pubsub_api_base(&gws_rust_core::validate::EndpointPolicy::google_only()),
            "https://pubsub.googleapis.com/v1"
        );
    }

    #[tokio::test]
    async fn pull_timeout_means_no_messages() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
            .mount(&server)
            .await;
        let client = PubSubClient::with_base(&Transport::for_test(&server.uri()), &server.uri());
        let got = client
            .pull("s", 1, Duration::from_millis(50))
            .await
            .unwrap();
        assert!(got.is_empty());
    }

    /// An idle long-poll whose headers arrive but whose body then stalls past
    /// the timeout is "no messages yet", the same as a wait for headers that
    /// times out: unacknowledged messages are redelivered, and the stall must
    /// not count against `--max-failures`.
    #[tokio::test]
    async fn pull_body_stall_means_no_messages() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let read = socket.read(&mut buf).await.unwrap();
            assert!(read > 0, "no request arrived");
            // Promise 100 bytes, send 1, then go silent with the socket open.
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n{")
                .await
                .unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        let client = PubSubClient::with_base(&Transport::for_test(&base), &base);
        let got = tokio::time::timeout(
            Duration::from_secs(5),
            client.pull("s", 1, Duration::from_millis(100)),
        )
        .await
        .expect("pull ignored its timeout while reading the body");
        assert!(matches!(got, Ok(ref v) if v.is_empty()), "{got:?}");
        server.abort();
    }

    /// A failed deletion is an API outcome with its own class (a refused
    /// permission exits 1), not an internal error (5); every failure is
    /// still named.
    #[tokio::test]
    async fn cleanup_failure_keeps_the_api_error_class() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/subscriptions/s"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": 403, "message": "denied", "status": "PERMISSION_DENIED"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/topics/t"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": {"code": 404, "message": "no such topic", "status": "NOT_FOUND"}
            })))
            .mount(&server)
            .await;
        let client = PubSubClient::with_base(&Transport::for_test(&server.uri()), &server.uri());
        let err = cleanup_resources(&client, &["subscriptions/s", "topics/t"])
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 403, .. }), "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
        let msg = err.to_string();
        for needle in ["subscriptions/s", "denied", "topics/t", "no such topic"] {
            assert!(msg.contains(needle), "{needle}: {msg}");
        }
    }

    #[tokio::test]
    async fn cleanup_reports_failures() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/topics/t"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/subscriptions/s"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let client = PubSubClient::with_base(&Transport::for_test(&server.uri()), &server.uri());
        let err = cleanup_resources(&client, &["subscriptions/s", "topics/t"])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("topics/t"));
        assert!(!err.to_string().contains("subscriptions/s ("));
    }
}
