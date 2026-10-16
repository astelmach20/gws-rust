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
//!   caps redirect chains. A redirect to another origin is followed by
//!   [`send`] itself, as a bare GET carrying no credentials, quota project or
//!   body, and refused when it would keep a method other than GET/HEAD.
//! * **One retry policy.** [`send`] retries transient failures with capped exponential
//!   backoff and full jitter, honours `Retry-After` in both its delta-seconds
//!   and HTTP-date forms, and never re-sends a non-idempotent request unless
//!   the failure proves the request never left this process.
//!
//! Which hosts may receive a bearer token is decided by
//! [`crate::validate::EndpointPolicy`].

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderName, HeaderValue, LOCATION, RANGE, RETRY_AFTER,
    USER_AGENT,
};
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

/// Redirect policy: follow at most [`MAX_REDIRECTS`] hops, never downgrade
/// from HTTPS to HTTP, and stop at a hop to another origin.
///
/// On a cross-origin hop `reqwest` removes only `Authorization`, `Cookie` and
/// `Proxy-Authorization`. It would still send `x-goog-user-project` and every
/// other header the caller set, and on a 307/308 the method and body too.
/// So such a hop is not followed here: the redirect response goes back to
/// [`send`], which follows it with [`follow_cross_origin`].
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
    let cross_origin = attempt
        .previous()
        .last()
        .is_some_and(|prev| prev.origin() != attempt.url().origin());
    if cross_origin {
        return attempt.stop();
    }
    attempt.follow()
}

/// Request headers carried to a redirect target on another origin: only
/// ones that describe the wanted representation. Everything else, such as
/// `x-goog-user-project`, upload metadata and any header a caller adds later,
/// stays with the origin the request was meant for. (`reqwest` adds the
/// client's default headers to every request itself.)
const CROSS_ORIGIN_HEADERS: [HeaderName; 4] = [ACCEPT, ACCEPT_LANGUAGE, RANGE, USER_AGENT];

/// The method a `status` redirect turns `method` into, as `reqwest` and
/// browsers apply RFC 9110: a 303 becomes a GET (a HEAD stays a HEAD), a
/// 301/302 turns a POST into a GET, and a 307/308 keeps the method.
fn redirected_method(status: StatusCode, method: &Method) -> Method {
    match status {
        StatusCode::SEE_OTHER if *method != Method::HEAD => Method::GET,
        StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND if *method == Method::POST => Method::GET,
        _ => method.clone(),
    }
}

/// Where `response` redirects to, when that is another origin. A response
/// that is not a followable redirect, or whose `Location` is not a valid URL
/// reference, is a final answer: `reqwest` returns those unfollowed too.
fn cross_origin_target(response: &reqwest::Response) -> Option<Url> {
    let redirect = matches!(
        response.status(),
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    );
    if !redirect {
        return None;
    }
    let location = response.headers().get(LOCATION)?.to_str().ok()?;
    let target = response.url().join(location).ok()?;
    (target.origin() != response.url().origin()).then_some(target)
}

/// Follow redirects to another origin that [`redirect_policy`] stopped at.
///
/// The request goes there as a fresh GET (or HEAD) with no body and only
/// [`CROSS_ORIGIN_HEADERS`]. A redirect that would keep any other method,
/// such as a 307/308 after a POST or PUT, is refused with
/// [`SendError::Redirect`] rather than followed. Following it would send
/// the request body (a message, a document, a permission change) to a host
/// the request was never checked against. Following it without the body
/// would turn the request into something else. Redirects within the new
/// origin are followed by `reqwest`, and the whole chain is capped at
/// [`MAX_REDIRECTS`] cross-origin hops.
async fn follow_cross_origin(
    client: &reqwest::Client,
    mut response: reqwest::Response,
    mut method: Method,
    headers: &HeaderMap,
    response_timeout: Option<Duration>,
) -> Result<reqwest::Response, SendError> {
    let mut hops = 0;
    while let Some(target) = cross_origin_target(&response) {
        let url = redact_url(response.url());
        let status = response.status();
        let refuse = |method: Method, reason: String| SendError::Redirect {
            method,
            url: url.clone(),
            reason,
        };
        if hops == MAX_REDIRECTS {
            return Err(refuse(
                method,
                format!("stopped after {MAX_REDIRECTS} redirects to other origins"),
            ));
        }
        hops += 1;
        let target_origin = target.origin().ascii_serialization();
        let next = redirected_method(status, &method);
        if next != Method::GET && next != Method::HEAD {
            return Err(refuse(
                method,
                format!(
                    "refusing to follow HTTP {} to another origin ({target_origin}): it would \
                     send the {next} request, and its body, to a host it was not meant for",
                    status.as_u16()
                ),
            ));
        }
        if response.url().scheme() == "https" && target.scheme() == "http" {
            return Err(refuse(
                method,
                format!("refusing HTTPS to HTTP redirect to {target_origin}"),
            ));
        }
        tracing::debug!(
            from = %url, to = %redact_url(&target), status = status.as_u16(),
            "following a redirect to another origin without credentials"
        );
        let mut rb = client.request(next.clone(), target.clone());
        for name in &CROSS_ORIGIN_HEADERS {
            for value in headers.get_all(name) {
                rb = rb.header(name, value);
            }
        }
        let sent =
            match response_timeout {
                Some(limit) => tokio::time::timeout(limit, rb.send()).await.map_err(|_| {
                    SendError::Timeout {
                        method: next.clone(),
                        url: redact_url(&target),
                        timeout: limit,
                        attempts: 1,
                        retried_allowed: true,
                    }
                })?,
                None => rb.send().await,
            };
        response = sent.map_err(|e| SendError::Transport {
            method: next.clone(),
            url: redact_url(&target),
            attempts: 1,
            retried_allowed: true,
            source: e.without_url(),
        })?;
        method = next;
    }
    Ok(response)
}

/// Builds a new client, with the same configuration as [`shared_client`] but
/// its own connection pool.
///
/// A pooled connection is driven by a task on the Tokio runtime that opened
/// it, so a pool must not outlive, or be shared across, runtimes. Code that
/// runs more than one runtime in a process (such as `#[tokio::test]`, which
/// starts a runtime per test) needs a client per runtime.
pub fn build_client() -> Result<reqwest::Client, GwsError> {
    build_client_inner().map_err(GwsError::other)
}

/// Returns a clone of the single process-wide client.
///
/// `reqwest::Client` is reference counted, so clones share one connection
/// pool. Building the client can only fail if the TLS backend cannot be
/// initialised; that failure is cached and reported on every call.
///
/// The pool is bound to the runtime that uses it first (see
/// [`build_client`]), which is correct for a binary that runs a single Tokio
/// runtime. This crate's unit tests start a runtime per test, so under
/// `cfg(test)` every call returns a new client instead.
pub fn shared_client() -> Result<reqwest::Client, GwsError> {
    if cfg!(test) {
        return build_client();
    }
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
    /// Whether a `method` request with this idempotency may be re-sent.
    pub fn resolve(self, method: &Method) -> bool {
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
    /// The `Retry-After` of the returned response, when it is a retryable
    /// failure that carried one. A `Retry-After` from an earlier attempt is
    /// never reported against a later, different answer.
    pub retry_after: Option<Duration>,
    /// Whether `response` is a retryable failure the client stopped retrying
    /// (attempts exhausted, a non-idempotent request, or a `Retry-After`
    /// longer than the policy allows). False for a definitive answer.
    pub gave_up: bool,
    /// Whether an earlier attempt may have reached the server and been
    /// applied without its answer arriving (a timeout or a retryable error
    /// status). A connection that could not be made does not count.
    pub earlier_attempt_unknown: bool,
    /// The request method.
    pub method: Method,
}

impl Sent {
    /// Human-readable note describing retries, for appending to error messages.
    /// `None` when the response is a definitive answer, even after earlier
    /// retried attempts.
    ///
    /// A DELETE that finds nothing (404/410) on a retry is noted too: an
    /// earlier attempt whose answer was lost may already have deleted it.
    pub fn retry_note(&self) -> Option<String> {
        if !self.gave_up {
            let not_found = matches!(
                self.response.status(),
                StatusCode::NOT_FOUND | StatusCode::GONE
            );
            return (self.method == Method::DELETE && not_found && self.earlier_attempt_unknown)
                .then(|| {
                    format!(
                        "an earlier attempt may already have deleted it: the request was retried \
                         after an attempt whose outcome is unknown, and attempt {} found nothing",
                        self.attempts
                    )
                });
        }
        retry_note(self.attempts, self.retry_after)
    }
}

/// The longest `Retry-After` a retry note states as a number of seconds.
/// Anything longer, including an overflowing header saturated to
/// [`Duration::MAX`] by [`parse_retry_after`], is no schedule worth printing.
const RETRY_AFTER_NOTE_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);

fn retry_after_note(retry_after: Duration) -> String {
    if retry_after > RETRY_AFTER_NOTE_LIMIT {
        format!(
            "server asked to retry after an unreasonably long wait (over {}s)",
            RETRY_AFTER_NOTE_LIMIT.as_secs()
        )
    } else {
        format!("server asked to retry after {}s", retry_after.as_secs())
    }
}

fn retry_note(attempts: u32, retry_after: Option<Duration>) -> Option<String> {
    match (attempts > 1, retry_after) {
        (false, None) => None,
        (true, None) => Some(format!("gave up after {attempts} attempts")),
        (false, Some(ra)) => Some(retry_after_note(ra)),
        (true, Some(ra)) => Some(format!(
            "gave up after {attempts} attempts; {}",
            retry_after_note(ra)
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

    #[error("{method} {url}: no data received for {}s while reading the HTTP {status} response body (see --timeout)", timeout.as_secs())]
    BodyTimeout {
        method: Method,
        url: String,
        status: StatusCode,
        timeout: Duration,
    },

    #[error("{method} {url}: failed to read HTTP {status} response body: {source}")]
    ReadBody {
        method: Method,
        url: String,
        status: StatusCode,
        #[source]
        source: reqwest::Error,
    },

    /// A redirect to another origin that is not followed (see [`send`]).
    #[error("{method} {url}: {reason}")]
    Redirect {
        method: Method,
        url: String,
        reason: String,
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
            | SendError::BodyTimeout { .. }
            | SendError::ReadBody { .. }
            | SendError::Redirect { .. } => GwsError::Network(Box::new(err)),
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

// Google signals per-user and per-project rate limits with HTTP 403 and a
// rate-limit reason in `error.errors[].reason` (or `error.details[].reason`).

/// Returns true when a Google error body names a retryable rate-limit reason.
pub fn is_rate_limit_body(body: &[u8]) -> bool {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    crate::error::rate_limit_reason(&json["error"]).is_some()
}

/// HTTP statuses retried for idempotent requests (the same set that exits
/// with code 6; see [`crate::error::is_retryable_status_code`]).
pub fn is_retryable_status(status: StatusCode) -> bool {
    crate::error::is_retryable_status_code(status.as_u16())
}

/// Parse a `Retry-After` header: delta-seconds or an HTTP-date. An invalid
/// value is `None`, which callers treat as "no hint" and fall back to their
/// own backoff (RFC 9110 lets recipients ignore an invalid Retry-After); a
/// date in the past means "retry now". Delta-seconds is digits only (RFC 9110
/// §10.2.3, so no sign); a value too large for `u64` is still a valid request
/// to wait and saturates rather than being ignored.
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        // All digits, so parsing can only fail by overflowing.
        return Some(
            value
                .parse::<u64>()
                .map_or(Duration::MAX, Duration::from_secs),
        );
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
    body_timeout: Option<Duration>,
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
    // body must be read and the response rebuilt around it. The read is
    // bounded by the response timeout: a body that stalls must not hang the
    // request past `--timeout`.
    let version = response.version();
    let headers = response.headers().clone();
    let read = response.bytes();
    let body = match body_timeout {
        Some(limit) => tokio::time::timeout(limit, read)
            .await
            .map_err(|_elapsed| SendError::BodyTimeout {
                method: method.clone(),
                url: url.to_string(),
                status,
                timeout: limit,
            })?,
        None => read.await,
    }
    .map_err(|source| SendError::ReadBody {
        method: method.clone(),
        url: url.to_string(),
        status,
        source: source.without_url(),
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
    let mut attempt: u32 = 0;
    // Set once an attempt may have been applied without its answer arriving.
    let mut earlier_attempt_unknown = false;
    loop {
        attempt += 1;
        let (client, request) = build().build_split();
        // reqwest errors print their full URL; the query is redacted below.
        let request = request.map_err(|e| SendError::Build(e.without_url()))?;
        let method = request.method().clone();
        let headers = request.headers().clone();
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
                // reqwest's message names the full URL, query string included.
                let err = err.without_url();
                // Connect errors mean nothing was sent; any method may retry.
                // An idempotent request also retries a connection that broke
                // after the request was written (a reset, or a close before
                // the response), which reqwest reports as a request error.
                let retryable =
                    err.is_connect() || (idempotent && (err.is_timeout() || err.is_request()));
                if retryable && can_retry {
                    earlier_attempt_unknown |= !err.is_connect();
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
                    earlier_attempt_unknown = true;
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

        let response = follow_cross_origin(
            &client,
            response,
            method.clone(),
            &headers,
            policy.response_timeout,
        )
        .await?;

        match classify(response, &method, &url, policy.response_timeout).await? {
            Verdict::Done(response) => {
                return Ok(Sent {
                    response,
                    attempts: attempt,
                    retry_after: None,
                    gave_up: false,
                    earlier_attempt_unknown,
                    method,
                });
            }
            Verdict::Retry {
                response,
                retry_after,
            } => {
                let too_long = retry_after.is_some_and(|ra| ra > policy.max_retry_after);
                if !idempotent || !can_retry || too_long {
                    return Ok(Sent {
                        response,
                        attempts: attempt,
                        retry_after,
                        gave_up: true,
                        earlier_attempt_unknown,
                        method,
                    });
                }
                let delay = retry_after.unwrap_or_else(|| policy.backoff(attempt - 1));
                // Saturating conversion for the log field only.
                tracing::debug!(
                    %method, %url, attempt, status = response.status().as_u16(),
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    "retrying after retryable status"
                );
                // A 5xx may come after the server applied the request; a 429,
                // 408 or rate-limit 403 means it was not processed.
                earlier_attempt_unknown |= response.status().is_server_error();
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

    /// A loopback port that refuses connections. Binding a listener and
    /// dropping it frees the port, and a server started by a test running in
    /// parallel can pick it up and answer. On Linux a socket that is bound
    /// but never listens keeps the port while still refusing connections, so
    /// the returned guard holds it for as long as it lives. (macOS drops the
    /// connection attempt instead of refusing it, so it keeps the bind-drop.)
    pub(crate) fn refused_port() -> (Option<tokio::net::TcpSocket>, u16) {
        if cfg!(target_os = "linux") {
            let socket = tokio::net::TcpSocket::new_v4().unwrap();
            socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
            let port = socket.local_addr().unwrap().port();
            return (Some(socket), port);
        }
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        (None, l.local_addr().unwrap().port())
    }

    /// Connecting to the port is refused, and on Linux no other listener can
    /// take it while the guard lives.
    #[tokio::test]
    async fn refused_port_refuses_and_stays_reserved() {
        let (reserved, port) = refused_port();
        if reserved.is_some() {
            assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());
        }
        let err = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
    }

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
        // Full jitter spreads over the whole range: 200 draws all landing
        // in the lower half (or at zero) has probability 2^-200.
        let longest = (0..200).map(|_| policy.backoff(1)).max().unwrap();
        assert!(
            longest > Duration::from_millis(100),
            "backoff never waits more than half its 200ms ceiling (longest {longest:?})"
        );
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

    /// A delta-seconds value too large for `u64` is still a valid (huge)
    /// request to wait, not a garbage value to ignore. Signs are not digits.
    #[test]
    fn retry_after_overflowing_seconds_is_huge_not_ignored() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(
            parse_retry_after("99999999999999999999999", now),
            Some(Duration::MAX)
        );
        assert_eq!(parse_retry_after("+5", now), None);
        assert_eq!(parse_retry_after("-5", now), None);
        assert_eq!(parse_retry_after("", now), None);
    }

    #[tokio::test]
    async fn overflowing_retry_after_stops_retrying() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(503).insert_header("retry-after", "99999999999999999999999"),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(sent.response.status(), 503);
        assert!(sent.gave_up);
        // The saturated wait is no schedule to print as a number.
        let note = sent.retry_note().unwrap();
        assert_eq!(
            note,
            "server asked to retry after an unreasonably long wait (over 86400s)"
        );
        assert!(!note.contains(&u64::MAX.to_string()), "{note}");
    }

    /// The retry note prints a plausible `Retry-After` in seconds, and one
    /// beyond a day (including a saturated, overflowing header) as "an
    /// unreasonably long wait" rather than a meaningless huge number.
    #[test]
    fn retry_note_caps_an_unreasonable_retry_after() {
        let day = Duration::from_secs(86_400);
        assert_eq!(
            retry_note(1, Some(day)).as_deref(),
            Some("server asked to retry after 86400s")
        );
        for huge in [day + Duration::from_secs(1), Duration::MAX] {
            assert_eq!(
                retry_note(1, Some(huge)).as_deref(),
                Some("server asked to retry after an unreasonably long wait (over 86400s)")
            );
            assert_eq!(
                retry_note(3, Some(huge)).as_deref(),
                Some(
                    "gave up after 3 attempts; server asked to retry after an unreasonably \
                     long wait (over 86400s)"
                )
            );
        }
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
    async fn final_non_retryable_response_carries_no_stale_retry_note() {
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(503).insert_header("Retry-After", "1"),
                ResponseTemplate::new(404).set_body_string("gone"),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(sent.attempts, 2);
        assert_eq!(sent.response.status(), 404);
        // The 404 is the server's definitive answer: it neither asked for a
        // retry nor did the client give up retrying it.
        assert_eq!(sent.retry_after, None);
        assert_eq!(sent.retry_note(), None);
    }

    /// A DELETE retried after a 5xx may already have been applied, so a
    /// 404/410 on the retry is noted. A 404 on the first attempt, or after
    /// a 429 (which was never processed), is a plain answer.
    #[tokio::test]
    async fn delete_not_found_after_an_unknown_attempt_is_noted() {
        let client = shared_client().unwrap();
        for (first, noted) in [(Some(503), true), (Some(429), false), (None, false)] {
            let server = MockServer::start().await;
            let mut responses: Vec<ResponseTemplate> =
                first.into_iter().map(ResponseTemplate::new).collect();
            responses.push(ResponseTemplate::new(410));
            mount_sequence(&server, "DELETE", responses).await;
            let url = format!("{}/r", server.uri());
            let sent = send(&fast_policy(), Idempotency::FromMethod, || {
                client.delete(&url)
            })
            .await
            .unwrap();
            assert_eq!(sent.response.status(), 410);
            let note = sent.retry_note();
            assert_eq!(
                note.as_deref()
                    .is_some_and(|n| n.contains("an earlier attempt may already have deleted it")),
                noted,
                "{first:?}: {note:?}"
            );
        }
    }

    #[tokio::test]
    async fn exhausted_retries_report_only_the_final_retry_after() {
        let server = MockServer::start().await;
        mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(503).insert_header("Retry-After", "1"),
                ResponseTemplate::new(503),
            ],
        )
        .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let policy = RetryPolicy {
            max_attempts: 2,
            ..fast_policy()
        };
        let sent = send(&policy, Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.response.status(), 503);
        assert_eq!(sent.retry_after, None);
        assert_eq!(
            sent.retry_note().as_deref(),
            Some("gave up after 2 attempts")
        );
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

        // A Retry-After within max_retry_after is waited out, although the
        // policy's own backoff is at most 5ms.
        let server = MockServer::start().await;
        let calls = mount_sequence(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(429).insert_header("retry-after", "1"),
                ResponseTemplate::new(200),
            ],
        )
        .await;
        let url = format!("{}/r", server.uri());
        let start = std::time::Instant::now();
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.response.status(), 200);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            start.elapsed() >= Duration::from_secs(1),
            "retried after {:?}, not after the server's Retry-After: 1",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn connection_refused_is_reported_with_attempts() {
        let (_reserved, port) = refused_port();
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

    /// reqwest's own error message names the full URL; the query string
    /// (an API key, a search term, a resumable upload_id) must not reach the
    /// error message or a debug log through it.
    #[tokio::test]
    async fn transport_error_does_not_echo_the_query_string() {
        let (_reserved, port) = refused_port();
        let client = shared_client().unwrap();
        let url = format!("http://127.0.0.1:{port}/r?key=FAKE-api-key&q=private-term");
        let err = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap_err();
        assert!(matches!(err, SendError::Transport { .. }), "{err}");
        let mut chain = vec![err.to_string(), format!("{err:?}")];
        let mut source = std::error::Error::source(&err);
        while let Some(e) = source {
            chain.push(e.to_string());
            source = e.source();
        }
        for text in chain {
            assert!(
                !text.contains("FAKE-api-key") && !text.contains("private-term"),
                "{text}"
            );
        }
        assert!(
            err.to_string().contains(&format!("127.0.0.1:{port}/r")),
            "{err}"
        );
    }

    /// A 403 is read to look for a rate-limit reason. A body that stops
    /// arriving is bounded by the response timeout instead of hanging the
    /// request forever.
    #[tokio::test]
    async fn stalled_forbidden_body_times_out() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/r", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let read = socket.read(&mut buf).await.unwrap();
            assert!(read > 0, "no request arrived");
            // Promise 100 bytes, send 8, then go silent with the socket open.
            socket
                .write_all(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 100\r\n\r\n{\"error\"")
                .await
                .unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        let client = shared_client().unwrap();
        let policy = RetryPolicy {
            response_timeout: Some(Duration::from_millis(200)),
            ..fast_policy()
        };
        let err = tokio::time::timeout(
            Duration::from_secs(5),
            send(&policy, Idempotency::FromMethod, || client.get(&url)),
        )
        .await
        .expect("send ignored the response timeout while reading a 403 body")
        .unwrap_err();
        assert!(
            matches!(err, SendError::BodyTimeout { status, .. } if status == 403),
            "{err}"
        );
        assert!(err.to_string().contains("--timeout"), "{err}");
        server.abort();
    }

    /// A local server that accepts each of the first `drops` connections,
    /// reads the request and closes the socket without answering (a
    /// connection reset after the request was sent), then answers 200.
    /// Returns its base URL and the number of connections accepted.
    async fn dropping_server(drops: u32) -> (String, Arc<AtomicU32>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let accepted = Arc::new(AtomicU32::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let n = counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let read = socket.read(&mut buf).await.unwrap();
                assert!(read > 0, "no request arrived");
                if n >= drops {
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                        )
                        .await
                        .unwrap();
                }
            }
        });
        (base, accepted)
    }

    /// A connection reset after the request was written is a connection
    /// error like any other: an idempotent request is retried, a POST is not
    /// (it may have reached the server).
    #[tokio::test]
    async fn connection_reset_is_retried_only_for_idempotent_requests() {
        let client = shared_client().unwrap();

        let (base, accepted) = dropping_server(1).await;
        let url = format!("{base}/r");
        let sent = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(sent.response.status(), 200);
        assert_eq!(sent.attempts, 2);
        assert!(sent.earlier_attempt_unknown);
        assert_eq!(accepted.load(Ordering::SeqCst), 2);

        let (base, accepted) = dropping_server(1).await;
        let url = format!("{base}/r");
        let err = send(&fast_policy(), Idempotency::FromMethod, || {
            client.post(&url)
        })
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                SendError::Transport {
                    attempts: 1,
                    retried_allowed: false,
                    ..
                }
            ),
            "{err}"
        );
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
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

    /// Two servers on different ports: `origin` redirects `/r` to `target`'s
    /// `/landing` with `status`.
    async fn cross_origin_redirect(status: u16) -> (MockServer, MockServer) {
        let target = MockServer::start().await;
        Mock::given(path("/landing"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&target)
            .await;
        let origin = MockServer::start().await;
        Mock::given(path("/r"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("location", format!("{}/landing", target.uri()).as_str()),
            )
            .mount(&origin)
            .await;
        (origin, target)
    }

    /// The quota project, and every other header that is not plainly about
    /// the content, stays with the origin the request was meant for.
    #[tokio::test]
    async fn quota_project_is_not_forwarded_across_origins() {
        for status in [301, 302, 303, 307, 308] {
            let (origin, target) = cross_origin_redirect(status).await;
            let client = shared_client().unwrap();
            let url = format!("{}/r", origin.uri());
            let sent = send(&fast_policy(), Idempotency::FromMethod, || {
                client
                    .get(&url)
                    .bearer_auth("secret-token")
                    .header("x-goog-user-project", "my-quota-project")
                    .header("x-upload-content-type", "text/plain")
                    .header("range", "bytes=0-9")
            })
            .await
            .unwrap();
            assert_eq!(sent.response.status(), 200, "{status}");
            let first = origin.received_requests().await.unwrap();
            assert!(first[0].headers.contains_key("x-goog-user-project"));
            let received = target.received_requests().await.unwrap();
            assert_eq!(received.len(), 1, "{status}");
            let headers = &received[0].headers;
            assert_eq!(received[0].method.as_str(), "GET", "{status}");
            for name in [
                "authorization",
                "x-goog-user-project",
                "x-upload-content-type",
            ] {
                assert!(!headers.contains_key(name), "{status}: {name} forwarded");
            }
            assert_eq!(headers.get("range").unwrap(), "bytes=0-9", "{status}");
        }
    }

    /// A 307/308 keeps the method and body. To another origin that would
    /// send the request body (a message, a document) somewhere it was not
    /// meant for, so the redirect is refused and nothing reaches the target.
    #[tokio::test]
    async fn cross_origin_307_308_with_a_body_is_refused() {
        for status in [307, 308] {
            let (origin, target) = cross_origin_redirect(status).await;
            let client = shared_client().unwrap();
            let url = format!("{}/r", origin.uri());
            let err = send(&fast_policy(), Idempotency::FromMethod, || {
                client
                    .post(&url)
                    .header("x-goog-user-project", "my-quota-project")
                    .json(&serde_json::json!({"raw": "private message"}))
            })
            .await
            .unwrap_err();
            assert!(
                matches!(err, SendError::Redirect { .. }),
                "{status}: {err:?}"
            );
            let text = err.to_string();
            assert!(text.contains("another origin"), "{text}");
            assert!(text.contains(&target.uri()), "{text}");
            assert!(
                target.received_requests().await.unwrap().is_empty(),
                "{status}: the body was re-sent"
            );
            assert!(matches!(GwsError::from(err), GwsError::Network(_)));
        }
    }

    /// A 303 (and a 301/302 after a POST) turns the request into a GET
    /// without a body, which may go to another origin, still without the
    /// quota project.
    #[tokio::test]
    async fn cross_origin_see_other_after_post_becomes_a_bare_get() {
        for status in [301, 302, 303] {
            let (origin, target) = cross_origin_redirect(status).await;
            let client = shared_client().unwrap();
            let url = format!("{}/r", origin.uri());
            let sent = send(&fast_policy(), Idempotency::FromMethod, || {
                client
                    .post(&url)
                    .header("x-goog-user-project", "my-quota-project")
                    .json(&serde_json::json!({"raw": "private message"}))
            })
            .await
            .unwrap();
            assert_eq!(sent.response.status(), 200, "{status}");
            let received = target.received_requests().await.unwrap();
            assert_eq!(received.len(), 1);
            assert_eq!(received[0].method.as_str(), "GET", "{status}");
            assert!(received[0].body.is_empty(), "{status}");
            assert!(!received[0].headers.contains_key("x-goog-user-project"));
            assert!(!received[0].headers.contains_key("content-type"));
        }
    }

    /// A PUT, PATCH or DELETE redirected to another origin is refused for
    /// every redirect status: it would keep its method there.
    #[tokio::test]
    async fn cross_origin_redirect_of_other_methods_is_refused() {
        let (origin, target) = cross_origin_redirect(302).await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", origin.uri());
        for m in [Method::PUT, Method::PATCH, Method::DELETE] {
            let err = send(&fast_policy(), Idempotency::FromMethod, || {
                client.request(m.clone(), &url).body("data")
            })
            .await
            .unwrap_err();
            assert!(matches!(err, SendError::Redirect { .. }), "{m}: {err:?}");
        }
        assert!(target.received_requests().await.unwrap().is_empty());
    }

    /// Redirects within one origin are followed as before, body and
    /// headers included.
    #[tokio::test]
    async fn same_origin_307_keeps_body_and_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/landing"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(path("/r"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", "/landing"))
            .mount(&server)
            .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", server.uri());
        let sent = send(&fast_policy(), Idempotency::FromMethod, || {
            client
                .post(&url)
                .bearer_auth("secret-token")
                .header("x-goog-user-project", "my-quota-project")
                .body("payload")
        })
        .await
        .unwrap();
        assert_eq!(sent.response.status(), 200);
        let received = server.received_requests().await.unwrap();
        let landing = received
            .iter()
            .find(|r| r.url.path() == "/landing")
            .unwrap();
        assert_eq!(landing.body, b"payload");
        assert!(landing.headers.contains_key("x-goog-user-project"));
        assert!(landing.headers.contains_key("authorization"));
    }

    /// A chain that keeps hopping between origins still stops.
    #[tokio::test]
    async fn cross_origin_redirect_loops_stop() {
        let a = MockServer::start().await;
        let b = MockServer::start().await;
        Mock::given(path("/r"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/r", b.uri()).as_str()),
            )
            .mount(&a)
            .await;
        Mock::given(path("/r"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/r", a.uri()).as_str()),
            )
            .mount(&b)
            .await;
        let client = shared_client().unwrap();
        let url = format!("{}/r", a.uri());
        let err = send(&fast_policy(), Idempotency::FromMethod, || client.get(&url))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("redirects"), "{err}");
    }
}
