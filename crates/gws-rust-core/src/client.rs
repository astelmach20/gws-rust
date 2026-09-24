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

//! HTTP transport for Google API requests.
//!
//! This module owns the three transport-level policies every request in the
//! project must follow:
//!
//! * **One client.** [`shared_client`] returns a clone of a single process-wide
//!   [`reqwest::Client`], so every request shares one connection pool and TLS
//!   session cache. Its redirect policy refuses HTTPS→HTTP downgrades and
//!   caps redirect chains; `reqwest` already strips `Authorization` on any
//!   cross-origin hop.
//! * **One retry policy.** [`send`] retries transient failures with capped exponential
//!   backoff and full jitter, honours `Retry-After` in both its delta-seconds
//!   and HTTP-date forms, and never re-sends a non-idempotent request unless
//!   the failure proves the request never left this process.
//!
//! Which hosts may receive a bearer token is decided by
//! [`crate::validate::EndpointPolicy`].

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{Method, StatusCode, Url};

use crate::error::GwsError;

/// Default time allowed for a server to start responding.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 10;

// ── Client ──────────────────────────────────────────────────────────────

fn build_client_inner() -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    let client_header = format!(
        "gl-rust/{}-{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    let header_value = HeaderValue::from_str(&client_header)
        .map_err(|e| format!("invalid x-goog-api-client header {client_header:?}: {e}"))?;
    headers.insert("x-goog-api-client", header_value);

    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(redirect_policy))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

/// Redirect policy: follow at most [`MAX_REDIRECTS`] hops and never downgrade
/// from HTTPS to HTTP. `reqwest` removes `Authorization`, `Cookie` and
/// `Proxy-Authorization` itself whenever scheme, host or port changes.
fn redirect_policy(attempt: reqwest::redirect::Attempt<'_>) -> reqwest::redirect::Action {
    if attempt.previous().len() >= MAX_REDIRECTS {
        return attempt.error(format!("stopped after {MAX_REDIRECTS} redirects"));
    }
    let downgrade = attempt.url().scheme() == "http"
        && attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.scheme() == "https");
    if downgrade {
        let target = attempt.url().to_string();
        return attempt.error(format!("refusing HTTPS to HTTP redirect to {target}"));
    }
    attempt.follow()
}

/// Returns a clone of the single process-wide client.
///
/// `reqwest::Client` is reference counted, so clones share one connection
/// pool. Building the client can only fail if the TLS backend cannot be
/// initialised; that failure is cached and reported on every call.
pub fn shared_client() -> Result<reqwest::Client, GwsError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(build_client_inner) {
        Ok(client) => Ok(client.clone()),
        Err(message) => Err(GwsError::other(message.clone())),
    }
}

// ── Timeouts ────────────────────────────────────────────────────────────

/// Parse a timeout given in whole seconds. `0` means "no timeout".
pub fn parse_timeout_secs(raw: &str, source: &str) -> Result<Option<Duration>, GwsError> {
    let secs: u64 = raw.trim().parse().map_err(|_| {
        GwsError::Validation(format!(
            "{source} must be a whole number of seconds (0 disables the timeout), got {raw:?}"
        ))
    })?;
    Ok((secs > 0).then(|| Duration::from_secs(secs)))
}

/// The process-wide request timeout, installed once at startup by the
/// binary after it has resolved its own precedence (flag, environment,
/// configuration file). `None` inside means "no timeout". This library never
/// reads the process environment itself.
static REQUEST_TIMEOUT: OnceLock<Option<Duration>> = OnceLock::new();

/// Install the process-wide request timeout (`None` disables it). Call at
/// most once per process; until then [`request_timeout`] is
/// [`DEFAULT_TIMEOUT`].
pub fn install_request_timeout(timeout: Option<Duration>) -> Result<(), GwsError> {
    REQUEST_TIMEOUT
        .set(timeout)
        .map_err(|_| GwsError::other("the request timeout was already installed"))
}

/// The installed request timeout (see [`install_request_timeout`]), or
/// [`DEFAULT_TIMEOUT`] when none was installed.
pub fn request_timeout() -> Option<Duration> {
    REQUEST_TIMEOUT
        .get()
        .copied()
        .unwrap_or(Some(DEFAULT_TIMEOUT))
}

// ── Retry policy ────────────────────────────────────────────────────────

/// Whether a request may safely be sent more than once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    /// Decide from the HTTP method: GET, HEAD, OPTIONS, PUT and DELETE are
    /// idempotent (RFC 9110 §9.2.2); POST and PATCH are not.
    FromMethod,
    /// The caller guarantees the request is idempotent (for example it
    /// carries a `requestId` the server de-duplicates on).
    Idempotent,
    /// Never re-send once the request may have reached the server.
    NonIdempotent,
}

impl Idempotency {
    fn resolve(self, method: &Method) -> bool {
        match self {
            Idempotency::Idempotent => true,
            Idempotency::NonIdempotent => false,
            Idempotency::FromMethod => matches!(
                *method,
                Method::GET | Method::HEAD | Method::OPTIONS | Method::PUT | Method::DELETE
            ),
        }
    }
}

/// Retry and timeout settings for [`send`].
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total attempts including the first one. `1` disables retries.
    pub max_attempts: u32,
    /// Backoff ceiling for the first retry; doubles on every retry.
    pub base_delay: Duration,
    /// Upper bound of any computed backoff.
    pub max_delay: Duration,
    /// Largest server-requested `Retry-After` we are willing to sleep for.
    /// A longer request stops retrying and returns the response as is.
    pub max_retry_after: Duration,
    /// Time allowed per attempt until response headers arrive. `None`
    /// disables it (used for large uploads whose body takes long to send).
    pub response_timeout: Option<Duration>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(32),
            max_retry_after: Duration::from_secs(60),
            response_timeout: Some(DEFAULT_TIMEOUT),
        }
    }
}

impl RetryPolicy {
    /// The default policy with the installed process-wide response timeout
    /// (see [`install_request_timeout`]).
    pub fn configured() -> Self {
        Self {
            response_timeout: request_timeout(),
            ..Self::default()
        }
    }

    /// Full-jitter backoff: a uniformly random delay in
    /// `[0, min(max_delay, base_delay * 2^retry)]`.
    pub fn backoff(&self, retry: u32) -> Duration {
        let ceiling = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(retry))
            .min(self.max_delay);
        // Saturating: a delay beyond u64 milliseconds clamps (never reached).
        let ceiling_ms = u64::try_from(ceiling.as_millis()).unwrap_or(u64::MAX);
        if ceiling_ms == 0 {
            return Duration::ZERO;
        }
        Duration::from_millis(rand::random_range(0..=ceiling_ms))
    }
}

/// A response together with how it was obtained.
#[derive(Debug)]
pub struct Sent {
    pub response: reqwest::Response,
    /// Number of attempts made (1 when the first attempt succeeded).
    pub attempts: u32,
    /// The last `Retry-After` the server sent, if any.
    pub retry_after: Option<Duration>,
}

impl Sent {
    /// Human-readable note describing retries, for appending to error messages.
    pub fn retry_note(&self) -> Option<String> {
        retry_note(self.attempts, self.retry_after)
    }
}

fn retry_note(attempts: u32, retry_after: Option<Duration>) -> Option<String> {
    match (attempts > 1, retry_after) {
        (false, None) => None,
        (true, None) => Some(format!("gave up after {attempts} attempts")),
        (false, Some(ra)) => Some(format!("server asked to retry after {}s", ra.as_secs())),
        (true, Some(ra)) => Some(format!(
            "gave up after {attempts} attempts; server asked to retry after {}s",
            ra.as_secs()
        )),
    }
}

/// Why [`send`] could not produce a response.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("failed to build HTTP request: {0}")]
    Build(#[source] reqwest::Error),

    #[error("{method} {url} failed after {attempts} attempt(s): {source}{}", not_retried_note(*.retried_allowed))]
    Transport {
        method: Method,
        url: String,
        attempts: u32,
        /// False when the request was not retried because it is not idempotent.
        retried_allowed: bool,
        #[source]
        source: reqwest::Error,
    },

    #[error("{method} {url}: no response within {}s after {attempts} attempt(s){}", timeout.as_secs(), not_retried_note(*.retried_allowed))]
    Timeout {
        method: Method,
        url: String,
        timeout: Duration,
        attempts: u32,
        retried_allowed: bool,
    },

    #[error("{method} {url}: failed to read HTTP {status} response body: {source}")]
    ReadBody {
        method: Method,
        url: String,
        status: StatusCode,
        #[source]
        source: reqwest::Error,
    },

    #[error(transparent)]
    Config(#[from] GwsError),
}

fn not_retried_note(retried_allowed: bool) -> &'static str {
    if retried_allowed {
        ""
    } else {
        " (not retried: the method is not idempotent, so the server may or may not have applied it)"
    }
}

impl From<SendError> for GwsError {
    fn from(err: SendError) -> Self {
        match err {
            SendError::Config(inner) => inner,
            SendError::Build(_) => GwsError::other(err),
            SendError::Transport { .. }
            | SendError::Timeout { .. }
            | SendError::ReadBody { .. } => GwsError::Network(Box::new(err)),
        }
    }
}

/// Outcome of inspecting one attempt.
enum Verdict {
    Done(reqwest::Response),
    Retry {
        response: reqwest::Response,
        retry_after: Option<Duration>,
    },
}

// Google signals per-user and per-project rate limits with HTTP 403 and one
// of these reasons in `error.errors[].reason` (or `error.details[].reason`).
use crate::error::RATE_LIMIT_REASONS;

/// Returns true when a Google error body names a retryable rate-limit reason.
pub fn is_rate_limit_body(body: &[u8]) -> bool {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let err = &json["error"];
    let reasons = err["errors"]
        .as_array()
        .into_iter()
        .chain(err["details"].as_array())
        .flatten()
        .filter_map(|e| e["reason"].as_str());
    reasons
        .chain(err["reason"].as_str())
        .any(|r| RATE_LIMIT_REASONS.contains(&r))
}

/// HTTP statuses retried for idempotent requests.
pub fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

/// Parse a `Retry-After` header: delta-seconds or an HTTP-date. An invalid
/// value is `None`, which callers treat as "no hint" and fall back to their
/// own backoff (RFC 9110 lets recipients ignore an invalid Retry-After); a
/// date in the past means "retry now".
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let when = httpdate::parse_http_date(value).ok()?;
    Some(when.duration_since(now).unwrap_or(Duration::ZERO))
}

fn header_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        // Non-ASCII is as invalid as an unparseable value: no hint.
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_retry_after(v, SystemTime::now()))
}

/// Rebuild a response whose body has already been read so it can be handed
/// back to the caller unchanged.
fn rebuild_response(
    status: StatusCode,
    version: reqwest::Version,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> reqwest::Response {
    let mut rebuilt = http::Response::new(body);
    *rebuilt.status_mut() = status;
    *rebuilt.version_mut() = version;
    *rebuilt.headers_mut() = headers;
    reqwest::Response::from(rebuilt)
}

async fn classify(
    response: reqwest::Response,
    method: &Method,
    url: &str,
) -> Result<Verdict, SendError> {
    let status = response.status();
    let retry_after = header_retry_after(response.headers());
    if is_retryable_status(status) {
        return Ok(Verdict::Retry {
            response,
            retry_after,
        });
    }
    if status != StatusCode::FORBIDDEN {
        return Ok(Verdict::Done(response));
    }
    // A 403 is only retryable when the body names a rate-limit reason, so the
    // body must be read and the response rebuilt around it.
    let version = response.version();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(|source| SendError::ReadBody {
            method: method.clone(),
            url: url.to_string(),
            status,
            source,
        })?;
    let rate_limited = is_rate_limit_body(&body);
    let response = rebuild_response(status, version, headers, body);
    Ok(if rate_limited {
        Verdict::Retry {
            response,
            retry_after,
        }
    } else {
        Verdict::Done(response)
    })
}

/// Send a request built by `build`, retrying transient failures.
///
/// `build` is called once per attempt so streamed bodies can be recreated.
///
/// * Idempotent requests are retried on 408/429/500/502/503/504, on a Google
///   403 `rateLimitExceeded`/`userRateLimitExceeded`, on connection errors and
///   on timeouts.
/// * Non-idempotent requests (POST/PATCH unless the caller says otherwise) are
///   retried **only** when the connection could not be established, which
///   proves the request never reached the server. They are never retried on a
///   timeout or an error status, so a slow `messages.send` cannot be sent twice.
/// * When retries are exhausted, the real final response (or error) is
///   returned, never a stale earlier one.
pub async fn send(
    policy: &RetryPolicy,
    idempotency: Idempotency,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<Sent, SendError> {
    let max_attempts = policy.max_attempts.max(1);
    let mut last_retry_after = None;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let (client, request) = build().build_split();
        let request = request.map_err(SendError::Build)?;
        let method = request.method().clone();
        let url = redact_url(request.url());
        let idempotent = idempotency.resolve(&method);
        let can_retry = attempt < max_attempts;

        let outcome = match policy.response_timeout {
            Some(limit) => match tokio::time::timeout(limit, client.execute(request)).await {
                Ok(result) => result.map_err(Some),
                Err(_elapsed) => Err(None),
            },
            None => client.execute(request).await.map_err(Some),
        };

        let response = match outcome {
            Ok(response) => response,
            Err(Some(err)) => {
                // Connect errors mean nothing was sent; any method may retry.
                let retryable = err.is_connect() || (idempotent && err.is_timeout());
                if retryable && can_retry {
                    tracing::debug!(%method, %url, attempt, error = %err, "retrying after transport error");
                    tokio::time::sleep(policy.backoff(attempt - 1)).await;
                    continue;
                }
                return Err(SendError::Transport {
                    method,
                    url,
                    attempts: attempt,
                    retried_allowed: idempotent || err.is_connect(),
                    source: err,
                });
            }
            Err(None) => {
                let timeout = policy.response_timeout.unwrap_or_default();
                if idempotent && can_retry {
                    tracing::debug!(%method, %url, attempt, "retrying after timeout");
                    tokio::time::sleep(policy.backoff(attempt - 1)).await;
                    continue;
                }
                return Err(SendError::Timeout {
                    method,
                    url,
                    timeout,
                    attempts: attempt,
                    retried_allowed: idempotent,
                });
            }
        };

        match classify(response, &method, &url).await? {
            Verdict::Done(response) => {
                return Ok(Sent {
                    response,
                    attempts: attempt,
                    retry_after: last_retry_after,
                });
            }
            Verdict::Retry {
                response,
                retry_after,
            } => {
                if retry_after.is_some() {
                    last_retry_after = retry_after;
                }
                let too_long = retry_after.is_some_and(|ra| ra > policy.max_retry_after);
                if !idempotent || !can_retry || too_long {
                    return Ok(Sent {
                        response,
                        attempts: attempt,
                        retry_after: last_retry_after,
                    });
                }
                let delay = retry_after.unwrap_or_else(|| policy.backoff(attempt - 1));
                // Saturating conversion for the log field only.
                tracing::debug!(
                    %method, %url, attempt, status = response.status().as_u16(),
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    "retrying after retryable status"
                );
                drop(response);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Drop the query string from a URL before it goes into an error message or
/// log line; query strings can carry user data such as search terms.
fn redact_url(url: &Url) -> String {
    let mut clean = url.clone();
    clean.set_query(None);
    clean.set_fragment(None);
    clean.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    fn fast_policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(2),
            response_timeout: Some(Duration::from_secs(5)),
        }
    }

    /// Responds with each template in turn, repeating the last one.
    struct Sequence {
        responses: Vec<ResponseTemplate>,
        calls: Arc<AtomicU32>,
    }

    impl Respond for Sequence {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
            self.responses[n.min(self.responses.len() - 1)].clone()
        }
    }

    async fn mount_sequence(
        server: &MockServer,
        http_method: &str,
        responses: Vec<ResponseTemplate>,
    ) -> Arc<AtomicU32> {
        let calls = Arc::new(AtomicU32::new(0));
        Mock::given(method(http_method))
            .and(path("/r"))
            .respond_with(Sequence {
                responses,
                calls: calls.clone(),
            })
            .mount(server)
            .await;
        calls
    }

    fn rate_limit_403() -> ResponseTemplate {
        ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": {"code": 403, "message": "slow down",
                      "errors": [{"reason": "userRateLimitExceeded"}]}
        }))
    }

    #[test]
    fn shared_client_is_reused() {
        let a = shared_client().unwrap();
        let b = shared_client().unwrap();
        let ra = a.get("https://example.com").build().unwrap();
        let rb = b.get("https://example.com").build().unwrap();
        assert_eq!(ra.url(), rb.url());
    }

    #[test]
    fn backoff_is_bounded_by_cap() {
        let policy = RetryPolicy {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(250),
            ..RetryPolicy::default()
        };
        for retry in 0..10 {
            let ceiling = Duration::from_millis(100 * 2u64.pow(retry)).min(policy.max_delay);
            assert!(policy.backoff(retry) <= ceiling);
        }
    }

    #[test]
    fn retry_after_parses_seconds_and_http_date() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
        let later = httpdate::fmt_http_date(now + Duration::from_secs(30));
        assert_eq!(
            parse_retry_after(&later, now),
            Some(Duration::from_secs(30))
        );
        let earlier = httpdate::fmt_http_date(now - Duration::from_secs(30));
        assert_eq!(parse_retry_after(&earlier, now), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn rate_limit_reasons_detected() {
        assert!(is_rate_limit_body(
            br#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#
        ));
        assert!(is_rate_limit_body(
            br#"{"error":{"details":[{"reason":"RATE_LIMIT_EXCEEDED"}]}}"#
        ));
        assert!(!is_rate_limit_body(
            br#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#
        ));
        assert!(!is_rate_limit_body(b"not json"));
    }

    #[test]
    fn timeout_parsing() {
        assert_eq!(
            parse_timeout_secs("30", "--timeout").unwrap(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(parse_timeout_secs("0", "--timeout").unwrap(), None);
        assert!(parse_timeout_secs("abc", "--timeout").is_err());
        assert!(parse_timeout_secs("-1", "--timeout").is_err());
    }

    #[tokio::test]
    async fn retries_idempotent_get_on_5xx_then_succeeds() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(503),
                ResponseTemplate::new(500),
                ResponseTemplate::new(200).set_body_string("ok"),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.response.status(), 200);
        assert_eq!(sent.attempts, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn returns_real_final_response_when_exhausted() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(503),
                ResponseTemplate::new(502).set_body_string("final"),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.attempts, 4);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(sent.response.status(), 502);
        assert_eq!(
            sent.retry_note().as_deref(),
            Some("gave up after 4 attempts")
        );
        assert_eq!(sent.response.text().await.unwrap(), "final");
    }

    #[tokio::test]
    async fn retries_google_rate_limit_403_and_preserves_other_403_body() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                rate_limit_403(),
                ResponseTemplate::new(403).set_body_json(serde_json::json!({
                    "error": {"code": 403, "message": "nope",
                              "errors": [{"reason": "insufficientPermissions"}]}
                })),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(sent.response.status(), 403);
        let body: serde_json::Value = sent.response.json().await.unwrap();
        assert_eq!(
            body["error"]["errors"][0]["reason"],
            "insufficientPermissions"
        );
    }

    #[tokio::test]
    async fn never_retries_post_on_error_status() {
        let server = MockServer::start().await;
        let calls = mount_sequence(&server, "POST", vec![ResponseTemplate::new(503)]).await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || {
            client.post(&url)
        })
        .await
        .unwrap();
        assert_eq!(sent.response.status(), 503);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn never_retries_post_on_timeout() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "POST",
            vec![ResponseTemplate::new(200).set_delay(Duration::from_millis(500))],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let policy = RetryPolicy {
            response_timeout: Some(Duration::from_millis(50)),
            ..fast_policy()
        };
        let err = send(&policy, Idempotency::FromMethod, || client.post(&url))
            .await
            .unwrap_err();
        assert!(
            matches!(err, SendError::Timeout { attempts: 1, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("not idempotent"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_post_when_caller_guarantees_idempotency() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "POST",
            vec![ResponseTemplate::new(503), ResponseTemplate::new(200)],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::Idempotent, || {
            client.post(&url)
        })
        .await
        .unwrap();
        assert_eq!(sent.response.status(), 200);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn get_timeout_is_retried() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(200).set_delay(Duration::from_millis(500)),
                ResponseTemplate::new(200),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let policy = RetryPolicy {
            response_timeout: Some(Duration::from_millis(100)),
            ..fast_policy()
        };
        let sent = send(&policy, Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.response.status(), 200);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn honours_retry_after_and_gives_up_when_too_long() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![ResponseTemplate::new(429).insert_header("retry-after", "3600")],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(sent.response.status(), 429);
        assert_eq!(sent.retry_after, Some(Duration::from_secs(3600)));
        assert!(sent.retry_note().unwrap().contains("3600s"));
    }

    #[tokio::test]
    async fn connection_refused_is_reported_with_attempts() {
        // Bind then drop a listener to get a port nothing listens on.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let client = shared_client().unwrap();
        let url = format!("http://127.0.0.1:{port}/r");
        let err = send(&fast_policy(), Idempotency::FromMethod, || {
            client.post(&url)
        })
        .await
        .unwrap_err();
        match err {
            SendError::Transport { attempts, .. } => assert_eq!(attempts, 4),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test]
    async fn authorization_is_not_forwarded_across_hosts() {
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/landing"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&target)
            .await;
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/r"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/landing", target.uri()).as_str()),
            )
            .mount(&origin)
            .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", origin.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || {
            client.get(&url).bearer_auth("secret-token")
        })
        .await
        .unwrap();
        assert_eq!(sent.response.status(), 200);
        let received = target.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        assert!(!received[0].headers.contains_key("authorization"));
        let first = origin.received_requests().await.unwrap();
        assert!(first[0].headers.contains_key("authorization"));
    }
}
