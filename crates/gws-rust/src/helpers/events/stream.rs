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
//!   exponential backoff and jitter. The loop exits with an error only after
//!   `--max-failures` consecutive failures, or immediately on a permanent
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
    /// Retry after a backoff (counts against the failure budget).
    Transient {
        status: Option<u16>,
        message: String,
    },
    /// Stop the loop and report this error.
    Fatal(GwsError),
}

/// Whether an HTTP status is worth retrying.
pub(crate) fn is_transient_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

impl From<GwsError> for StepError {
    /// API errors are transient only for retryable statuses; validation and
    /// discovery errors are permanent; anything else (network failures,
    /// token refresh failures, unreadable bodies) is treated as transient and
    /// bounded by the failure budget.
    fn from(e: GwsError) -> Self {
        match &e {
            GwsError::Api { code, .. } if is_transient_status(*code) => StepError::Transient {
                status: Some(*code),
                message: e.to_string(),
            },
            GwsError::Api { .. } | GwsError::Validation(_) | GwsError::Discovery(_) => {
                StepError::Fatal(e)
            }
            _ => StepError::Transient {
                status: None,
                message: e.to_string(),
            },
        }
    }
}

/// Write one diagnostic record to stderr as a single JSON line.
pub(crate) fn emit_diagnostic(level: &str, event: &str, fields: Value) {
    let mut record = json!({ "level": level, "event": event });
    if let (Some(obj), Value::Object(extra)) = (record.as_object_mut(), fields) {
        obj.extend(extra);
    }
    crate::output::eprint_line(&format!("{record}"));
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

/// One iteration of a pull loop.
pub(crate) trait StreamStep {
    async fn step(&mut self) -> Result<(), StepError>;
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
        let result = tokio::select! {
            r = step.step() => r,
            _ = crate::helpers::shutdown_signal() => {
                emit_diagnostic("info", "shutdown", json!({}));
                return Ok(());
            }
        };
        match result {
            Ok(()) => {
                budget.record_success();
                if opts.once {
                    return Ok(());
                }
                if !sleep_or_shutdown(opts.poll_interval).await {
                    return Ok(());
                }
            }
            Err(StepError::Fatal(e)) => return Err(e),
            Err(StepError::Transient { status, message }) => {
                let Some(delay) = budget.record_failure() else {
                    let summary = format!(
                        "giving up after {} consecutive transient failures; last error: {message}",
                        budget.consecutive() - 1
                    );
                    return Err(match status {
                        Some(code) => GwsError::Api {
                            code,
                            message: summary,
                            reason: "transientFailureBudgetExhausted".to_string(),
                            enable_url: None,
                        },
                        None => GwsError::other(summary),
                    });
                };
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
    ) -> Result<Value, StepError> {
        self.transport
            .json(method, &url, &[], body, Idempotency::FromMethod, context)
            .await
            .map_err(StepError::from)
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
        .map_err(into_gws)
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
        .map_err(into_gws)
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
        .map_err(into_gws)
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
        .map_err(into_gws)
    }
}

/// Collapse a `StepError` into a `GwsError` for one-shot setup calls.
pub(crate) fn into_gws(e: StepError) -> GwsError {
    match e {
        StepError::Fatal(e) => e,
        StepError::Transient {
            status: Some(code),
            message,
        } => GwsError::Api {
            code,
            message,
            reason: "transientError".to_string(),
            enable_url: None,
        },
        StepError::Transient {
            status: None,
            message,
        } => GwsError::other(message),
    }
}

/// Delete created Pub/Sub resources, reporting every failure. Returns an
/// error listing the resources that could not be deleted.
pub(crate) async fn cleanup_resources(
    client: &PubSubClient,
    names: &[&str],
) -> Result<(), GwsError> {
    let mut failed = Vec::new();
    for name in names {
        match client.delete(name).await {
            Ok(()) => emit_diagnostic("info", "deleted", json!({ "resource": name })),
            Err(e) => failed.push(format!("{name} ({e})")),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(GwsError::other(format!(
            "Failed to delete Pub/Sub resources: {}",
            failed.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn transient_classification() {
        for code in [408, 429, 500, 502, 503, 504] {
            assert!(is_transient_status(code));
        }
        for code in [400, 401, 403, 404, 409] {
            assert!(!is_transient_status(code));
        }
        let e = GwsError::Api {
            code: 503,
            message: "x".into(),
            reason: "r".into(),
            enable_url: None,
        };
        assert!(matches!(
            StepError::from(e),
            StepError::Transient {
                status: Some(503),
                ..
            }
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
            StepError::Transient { status: None, .. }
        ));
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
        async fn step(&mut self) -> Result<(), StepError> {
            self.calls += 1;
            self.results.pop_front().unwrap_or(Ok(()))
        }
    }

    fn transient(code: u16) -> Result<(), StepError> {
        Err(StepError::Transient {
            status: Some(code),
            message: format!("HTTP {code}"),
        })
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
        match err {
            GwsError::Api { code, message, .. } => {
                assert_eq!(code, 502);
                assert!(message.contains("giving up after 2"));
            }
            other => panic!("unexpected {other:?}"),
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
            StepError::Transient {
                status: Some(503),
                ..
            }
        ));
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
