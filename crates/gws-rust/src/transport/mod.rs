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

//! The one credential-attaching request path, shared by the executor and
//! every helper.
//!
//! [`Transport::send`] is the only function that puts a bearer token on a
//! request. It validates the destination with [`EndpointPolicy::check`]
//! first, sends through core's retry policy ([`gws_rust_core::client::send`];
//! a non-idempotent request is sent at most once), and on a 401 mints a new
//! token once through [`AccessTokenProvider::refresh_access_token`].
//! Non-2xx responses become [`GwsError::Api`] through [`errors::error_from_response`].

pub(crate) mod errors;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Method, StatusCode};
use serde_json::Value;

use gws_rust_core::client::{Idempotency, RetryPolicy, Sent};
use gws_rust_core::validate::EndpointPolicy;

use crate::auth::AccessTokenProvider;
use crate::error::GwsError;

/// Tracks what authentication method was used for the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// OAuth2 bearer token that is refreshed once after a 401.
    OAuth,
    /// A fixed bearer token (`GWSR_TOKEN`) that gwsr cannot refresh.
    FixedToken,
    /// No authentication was provided.
    None,
}

/// How requests are authenticated.
#[derive(Clone)]
pub enum Credentials {
    /// Unauthenticated requests.
    None,
    /// A fixed access token that cannot be refreshed (`GWSR_TOKEN`): a 401
    /// is final.
    Static(String),
    /// A token that is refreshed through `provider` when the API answers 401.
    Refreshable {
        token: String,
        provider: Arc<dyn AccessTokenProvider>,
    },
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Credentials::None => "None",
            Credentials::Static(_) => "Static(<redacted>)",
            Credentials::Refreshable { .. } => "Refreshable(<redacted>)",
        };
        f.write_str(kind)
    }
}

impl Credentials {
    pub(crate) fn auth_method(&self) -> AuthMethod {
        match self {
            Credentials::None => AuthMethod::None,
            Credentials::Static(_) => AuthMethod::FixedToken,
            Credentials::Refreshable { .. } => AuthMethod::OAuth,
        }
    }

    /// A token for `scopes`: refreshable, unless the source is a fixed token
    /// (`GWSR_TOKEN`). Only a complete absence of credentials
    /// ([`crate::auth::AuthError::NoCredentials`]) falls back to
    /// unauthenticated requests; every other failure is reported.
    pub(crate) async fn for_scopes(scopes: &[String]) -> Result<Self, GwsError> {
        let scopes: Vec<&str> = scopes.iter().map(String::as_str).collect();
        match crate::auth::get_token(&scopes).await {
            Ok(token) => {
                if crate::auth::active_source_can_refresh().map_err(crate::auth::to_gws_error)? {
                    Ok(Credentials::Refreshable {
                        token,
                        provider: Arc::new(crate::auth::token_provider(&scopes)),
                    })
                } else {
                    Ok(Credentials::Static(token))
                }
            }
            Err(e)
                if matches!(
                    e.downcast_ref::<crate::auth::AuthError>(),
                    Some(crate::auth::AuthError::NoCredentials)
                ) =>
            {
                Ok(Credentials::None)
            }
            Err(e) => Err(crate::auth::to_gws_error(e)),
        }
    }

    /// Like [`Credentials::for_scopes`], but a missing credential is an error:
    /// helpers always call APIs that need one.
    pub(crate) async fn required_for_scopes(scopes: &[&str]) -> Result<Self, GwsError> {
        let owned: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();
        match Self::for_scopes(&owned).await? {
            Credentials::None => Err(GwsError::Auth(format!(
                "No credentials found (needed for scopes: {}). Run `gwsr auth login`.",
                scopes.join(" ")
            ))),
            creds => Ok(creds),
        }
    }
}

/// The quota project to bill requests to, unless `GWSR_NO_QUOTA_PROJECT` is set.
pub(crate) fn quota_project_from_env() -> Result<Option<String>, GwsError> {
    if crate::env::get()?.no_quota_project {
        return Ok(None);
    }
    crate::auth::get_quota_project()
}

struct Shared {
    client: reqwest::Client,
    endpoints: EndpointPolicy,
    token: Mutex<Option<String>>,
    provider: Option<Arc<dyn AccessTokenProvider>>,
    quota_project: Option<String>,
}

/// Authenticated HTTP access to Google APIs. Cheap to clone; clones share
/// the token, so a refresh on one is seen by all.
#[derive(Clone)]
pub(crate) struct Transport {
    shared: Arc<Shared>,
    pub(crate) retry: RetryPolicy,
}

impl Transport {
    pub(crate) fn new(
        credentials: &Credentials,
        retry: RetryPolicy,
        endpoints: EndpointPolicy,
        quota_project: Option<String>,
    ) -> Result<Self, GwsError> {
        let (token, provider) = match credentials {
            Credentials::None => (None, None),
            Credentials::Static(t) => (Some(t.clone()), None),
            Credentials::Refreshable { token, provider } => {
                (Some(token.clone()), Some(provider.clone()))
            }
        };
        // A quota project only makes sense on an authenticated request.
        let quota_project = quota_project.filter(|_| token.is_some());
        Ok(Self {
            shared: Arc::new(Shared {
                client: http_client()?,
                endpoints,
                token: Mutex::new(token),
                provider,
                quota_project,
            }),
            retry,
        })
    }

    /// A transport with `credentials` and the environment's retry, endpoint
    /// and quota-project settings.
    pub(crate) fn from_env(credentials: &Credentials) -> Result<Self, GwsError> {
        Self::new(
            credentials,
            RetryPolicy::configured(),
            crate::env::get()?.endpoint_policy(),
            quota_project_from_env()?,
        )
    }

    /// A transport with no credentials (used to render dry-run requests; a
    /// request sent through it carries no token).
    pub(crate) fn unauthenticated() -> Result<Self, GwsError> {
        Self::new(
            &Credentials::None,
            RetryPolicy::configured(),
            crate::env::get()?.endpoint_policy(),
            None,
        )
    }

    /// An authenticated transport for `scopes` (credentials are required).
    /// Helpers use it; error hints name `scopes` as what to log in for.
    pub(crate) async fn for_scopes(scopes: &[&str]) -> Result<Self, GwsError> {
        crate::error::note_helper_scopes(scopes);
        Self::from_env(&Credentials::required_for_scopes(scopes).await?)
    }

    /// This transport with a different retry policy (e.g. a single attempt
    /// for streaming loops that apply their own backoff).
    pub(crate) fn with_retry(&self, retry: RetryPolicy) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            retry,
        }
    }

    /// A transport for tests: a static token, fast retries, and `base`
    /// (typically a wiremock server) trusted as the endpoint override.
    #[cfg(test)]
    pub(crate) fn for_test(base: &str) -> Self {
        Self::new(
            &Credentials::Static("test-token".to_string()),
            RetryPolicy {
                max_attempts: 2,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
                max_retry_after: Duration::ZERO,
                response_timeout: Some(Duration::from_secs(10)),
            },
            EndpointPolicy::with_override(base).expect("test endpoint override"),
            None,
        )
        .expect("test transport")
    }

    pub(crate) fn endpoints(&self) -> &EndpointPolicy {
        &self.shared.endpoints
    }

    pub(crate) fn auth_method(&self) -> Result<AuthMethod, GwsError> {
        Ok(match (self.current_token()?, &self.shared.provider) {
            (None, _) => AuthMethod::None,
            (Some(_), None) => AuthMethod::FixedToken,
            (Some(_), Some(_)) => AuthMethod::OAuth,
        })
    }

    fn current_token(&self) -> Result<Option<String>, GwsError> {
        self.shared
            .token
            .lock()
            .map(|t| t.clone())
            .map_err(|_| GwsError::other("access token lock poisoned"))
    }

    async fn refresh_token(&self) -> Result<(), GwsError> {
        let Some(provider) = &self.shared.provider else {
            return Ok(());
        };
        let fresh = provider.refresh_access_token().await.map_err(|e| {
            crate::auth::to_gws_error(
                e.context("failed to refresh the access token after HTTP 401"),
            )
        })?;
        let mut guard = self
            .shared
            .token
            .lock()
            .map_err(|_| GwsError::other("access token lock poisoned"))?;
        *guard = Some(fresh);
        Ok(())
    }

    /// Send `method url` with this transport's retry policy.
    pub(crate) async fn send(
        &self,
        method: Method,
        url: &str,
        idempotency: Idempotency,
        customize: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Sent, GwsError> {
        let policy = self.retry.clone();
        self.send_with_policy(&policy, method, url, idempotency, customize)
            .await
    }

    /// Send `method url` with an explicit retry policy.
    ///
    /// The URL is validated before any credential is attached; a URL that is
    /// not https on `*.googleapis.com` (or the configured override origin) is
    /// rejected and nothing is sent.
    pub(crate) async fn send_with_policy(
        &self,
        policy: &RetryPolicy,
        method: Method,
        url: &str,
        idempotency: Idempotency,
        customize: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Sent, GwsError> {
        let url = self.shared.endpoints.check(url)?;
        let mut refreshed = false;
        // What the attempts before a 401 refresh did: the returned `Sent`
        // describes the whole request, so a retry note still counts them and
        // still says when one of them may have been applied.
        let mut earlier_attempts: u32 = 0;
        let mut earlier_unknown = false;
        loop {
            let token = self.current_token()?;
            let mut sent = gws_rust_core::client::send(policy, idempotency, || {
                let mut rb = self.shared.client.request(method.clone(), url.clone());
                if let Some(token) = &token {
                    rb = rb.bearer_auth(token);
                }
                if let Some(project) = &self.shared.quota_project {
                    rb = rb.header("x-goog-user-project", project);
                }
                customize(rb)
            })
            .await?;
            sent.attempts = sent.attempts.saturating_add(earlier_attempts);
            sent.earlier_attempt_unknown |= earlier_unknown;
            // A 401 means the server rejected the credential before doing any
            // work, so re-sending (even a POST) cannot duplicate an effect.
            if sent.response.status() == StatusCode::UNAUTHORIZED
                && self.shared.provider.is_some()
                && !refreshed
            {
                // No query string: a resumable session URL carries its
                // upload_id there, and file logs record debug lines.
                let mut logged = url.clone();
                logged.set_query(None);
                logged.set_fragment(None);
                tracing::debug!(url = %logged, "HTTP 401; refreshing access token and retrying once");
                refreshed = true;
                earlier_attempts = sent.attempts;
                earlier_unknown = sent.earlier_attempt_unknown;
                self.refresh_token().await?;
                continue;
            }
            return Ok(sent);
        }
    }

    /// Turn a non-success response into an error, prefixing `context`.
    pub(crate) async fn error_for(&self, sent: Sent, context: &str) -> GwsError {
        let status = sent.response.status();
        let note = sent.retry_note();
        let auth = match self.auth_method() {
            Ok(a) => a,
            Err(e) => return e,
        };
        let err = match read_body(sent.response, self.retry.response_timeout).await {
            Ok(body) => errors::error_from_response(
                status,
                &String::from_utf8_lossy(&body),
                &auth,
                note.as_deref(),
            ),
            Err(e) => GwsError::Api {
                code: status.as_u16(),
                message: format!("HTTP {status} (error body unreadable: {e})"),
                reason: "httpError".to_string(),
                enable_url: None,
            },
        };
        errors::with_context(err, context)
    }

    /// Send and require a 2xx response; other statuses become
    /// [`GwsError::Api`] prefixed with `context`.
    pub(crate) async fn send_ok(
        &self,
        method: Method,
        url: &str,
        idempotency: Idempotency,
        context: &str,
        customize: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, GwsError> {
        let sent = self
            .send(method, url, idempotency, customize)
            .await
            .map_err(|e| errors::with_context(e, context))?;
        if sent.response.status().is_success() {
            return Ok(sent.response);
        }
        Err(self.error_for(sent, context).await)
    }

    /// Send a JSON request and parse the JSON response. Empty bodies (e.g.
    /// 204) become `{}`.
    pub(crate) async fn json(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        idempotency: Idempotency,
        context: &str,
    ) -> Result<Value, GwsError> {
        let needs_length = matches!(method, Method::POST | Method::PUT | Method::PATCH);
        let resp = self
            .send_ok(method, url, idempotency, context, |rb| {
                let rb = if query.is_empty() {
                    rb
                } else {
                    rb.query(query)
                };
                match body {
                    Some(b) => rb.json(b),
                    None if needs_length => rb.header(reqwest::header::CONTENT_LENGTH, 0),
                    None => rb,
                }
            })
            .await?;
        json_body(resp, self.retry.response_timeout, context).await
    }

    /// Idempotent GET returning JSON.
    pub(crate) async fn get_json(
        &self,
        url: &str,
        query: &[(&str, String)],
        context: &str,
    ) -> Result<Value, GwsError> {
        self.json(
            Method::GET,
            url,
            query,
            None,
            Idempotency::Idempotent,
            context,
        )
        .await
    }
}

/// The HTTP client for a new [`Transport`].
///
/// `gwsr` runs a single Tokio runtime, so every transport shares the
/// process-wide connection pool. Unit tests start a runtime per
/// `#[tokio::test]`, and a pooled connection is driven by a task on the
/// runtime that opened it; sharing the pool let one test pick up another
/// test's connection and fail with "runtime dropped the dispatch task". Under
/// `cfg(test)` each transport therefore gets its own pool.
fn http_client() -> Result<reqwest::Client, GwsError> {
    if cfg!(test) {
        crate::client::build_client()
    } else {
        crate::client::shared_client()
    }
}

/// Whether `err` is a request that got no response within the configured
/// response timeout: no headers in time, or a body that stopped arriving for
/// that long.
pub(crate) fn is_timeout(err: &GwsError) -> bool {
    use gws_rust_core::client::SendError;
    let (GwsError::Network(e) | GwsError::Other(e)) = err else {
        return false;
    };
    let root: &(dyn std::error::Error + 'static) = e.as_ref();
    // A connect timeout is a connection that could not be made: the request
    // was never sent, so it is not "no response within the timeout".
    let no_response = |e: &reqwest::Error| e.is_timeout() && !e.is_connect();
    std::iter::successors(Some(root), |e| e.source()).any(|e| {
        e.is::<BodyIdleTimeout>()
            || match e.downcast_ref::<SendError>() {
                Some(SendError::Timeout { .. }) => true,
                Some(SendError::Transport { source, .. }) => no_response(source),
                _ => e.downcast_ref::<reqwest::Error>().is_some_and(no_response),
            }
    })
}

/// Read a successful response as JSON. Empty bodies become `{}`.
pub(crate) async fn json_body(
    resp: reqwest::Response,
    idle: Option<Duration>,
    context: &str,
) -> Result<Value, GwsError> {
    let raw = read_body(resp, idle)
        .await
        .map_err(|e| errors::with_context(e, context))?;
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_slice(&raw).map_err(|e| {
        GwsError::other(anyhow::anyhow!(
            "{context}: response is not valid JSON: {e}"
        ))
    })
}

/// Read a response body, failing if no bytes arrive for `idle` (when set).
pub(crate) async fn read_body(
    response: reqwest::Response,
    idle: Option<Duration>,
) -> Result<Vec<u8>, GwsError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(&mut stream, idle).await? {
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// No bytes of a response body arrived within the idle limit. [`is_timeout`]
/// counts it as "no response within the timeout", like a wait for headers
/// that times out.
#[derive(Debug, thiserror::Error)]
#[error("no data received for {}s while reading the response body (see --timeout)", .0.as_secs())]
pub(crate) struct BodyIdleTimeout(Duration);

/// Next chunk of a body stream with an idle timeout.
pub(crate) async fn next_chunk<S>(
    stream: &mut S,
    idle: Option<Duration>,
) -> Result<Option<bytes::Bytes>, GwsError>
where
    S: futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let next = match idle {
        Some(limit) => tokio::time::timeout(limit, stream.next())
            .await
            .map_err(|_| GwsError::Network(Box::new(BodyIdleTimeout(limit))))?,
        None => stream.next().await,
    };
    next.transpose().map_err(|e| {
        GwsError::Network(
            anyhow::Error::new(e)
                .context("failed to read response body")
                .into(),
        )
    })
}

/// Timeout for a request that uploads `bytes`: the base response timeout plus
/// one second per 64 KiB, so slow links are not cut off mid-upload.
/// Saturating: a huge `--timeout` clamps instead of overflowing.
pub(crate) fn upload_timeout(base: Option<Duration>, bytes: u64) -> Option<Duration> {
    base.map(|b| b.saturating_add(Duration::from_secs(bytes / (64 * 1024))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn json_attaches_token_and_parses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/x"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;
        let t = Transport::for_test(&server.uri());
        let v = t
            .get_json(&format!("{}/v1/x", server.uri()), &[], "get x")
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn unreachable_server_is_a_network_error_with_context() {
        // Bind then drop a listener so the port is closed.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let base = format!("http://127.0.0.1:{port}");
        let t = Transport::for_test(&base);
        let err = t
            .get_json(&format!("{base}/v1/x"), &[], "get x")
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Network(_)), "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_NETWORK);
        assert!(err.to_string().starts_with("get x"), "{err}");
        assert!(!is_timeout(&err));
    }

    /// A Pub/Sub pull that times out means "no messages yet", but a connect
    /// timeout means the host could not be reached and nothing was sent: it
    /// must surface as a network error, not as an empty pull.
    #[tokio::test]
    async fn connect_timeout_is_not_a_response_timeout() {
        struct NeverResolves;
        impl reqwest::dns::Resolve for NeverResolves {
            fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
                Box::pin(std::future::pending())
            }
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(std::sync::Arc::new(NeverResolves))
            .connect_timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        let policy = gws_rust_core::client::RetryPolicy {
            max_attempts: 1,
            response_timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        };
        let err = gws_rust_core::client::send(&policy, Idempotency::Idempotent, || {
            client.get("https://pubsub.invalid/v1/x")
        })
        .await
        .unwrap_err();
        let gws = GwsError::from(err);
        assert!(matches!(gws, GwsError::Network(_)), "{gws:?}");
        assert!(
            !is_timeout(&gws),
            "a connect timeout read as no response: {gws}"
        );
    }

    #[tokio::test]
    async fn errors_carry_status_reason_and_context() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": {"code": 404, "message": "Not Found", "errors": [{"reason": "notFound"}]}
            })))
            .mount(&server)
            .await;
        let t = Transport::for_test(&server.uri());
        match t
            .get_json(&format!("{}/v1/x", server.uri()), &[], "Failed to get x")
            .await
            .unwrap_err()
        {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(code, 404);
                assert_eq!(reason, "notFound");
                assert!(message.starts_with("Failed to get x: "), "{message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_idempotent_requests_are_sent_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let t = Transport::for_test(&server.uri());
        let err = t
            .json(
                Method::POST,
                &format!("{}/v1/send", server.uri()),
                &[],
                Some(&json!({})),
                Idempotency::NonIdempotent,
                "send",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 503, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn idempotent_requests_are_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&server)
            .await;
        let t = Transport::for_test(&server.uri());
        assert!(
            t.json(
                Method::POST,
                &format!("{}/v1/modify", server.uri()),
                &[],
                None,
                Idempotency::Idempotent,
                "modify",
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn untrusted_urls_get_no_request() {
        let server = MockServer::start().await;
        let t = Transport::for_test(&server.uri());
        let err = t
            .get_json("https://evil.example/x", &[], "x")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Refusing"), "{err}");
    }

    #[tokio::test]
    async fn non_idempotent_request_is_not_resent_after_a_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
            .expect(1)
            .mount(&server)
            .await;
        let base = Transport::for_test(&server.uri());
        let t = base.with_retry(RetryPolicy {
            response_timeout: Some(Duration::from_millis(50)),
            ..base.retry.clone()
        });
        assert!(
            t.json(
                Method::POST,
                &format!("{}/v1/send", server.uri()),
                &[],
                None,
                Idempotency::NonIdempotent,
                "send",
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn empty_success_body_is_an_empty_object_and_bad_json_fails() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        let t = Transport::for_test(&server.uri());
        let url = format!("{}/v1/x", server.uri());
        let v = t
            .json(
                Method::DELETE,
                &url,
                &[],
                None,
                Idempotency::Idempotent,
                "del",
            )
            .await
            .unwrap();
        assert_eq!(v, json!({}));
        let err = t.get_json(&url, &[], "get").await.unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
    }

    /// A token the API still rejects after one refresh is an invalid or
    /// expired credential: exit code 2 ("Auth error: credentials missing,
    /// expired or invalid"), not the permanent-API-error code 1.
    #[tokio::test]
    async fn rejected_token_after_refresh_is_an_auth_error() {
        struct Provider(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl AccessTokenProvider for Provider {
            async fn refresh_access_token(&self) -> anyhow::Result<String> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok("fresh-token".to_string())
            }
        }
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {"code": 401,
                          "message": "Request had invalid authentication credentials.",
                          "errors": [{"reason": "authError"}],
                          "status": "UNAUTHENTICATED"}
            })))
            .expect(2)
            .mount(&server)
            .await;
        let provider = Arc::new(Provider(std::sync::atomic::AtomicUsize::new(0)));
        let t = Transport::new(
            &Credentials::Refreshable {
                token: "stale-token".to_string(),
                provider: provider.clone(),
            },
            Transport::for_test(&server.uri()).retry,
            EndpointPolicy::with_override(&server.uri()).unwrap(),
            None,
        )
        .unwrap();
        let err = t
            .get_json(&format!("{}/v1/x", server.uri()), &[], "Failed to get x")
            .await
            .unwrap_err();
        assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_AUTH, "{err:?}");
        assert!(
            err.to_string()
                .contains("Request had invalid authentication credentials."),
            "{err}"
        );
    }

    /// A DELETE whose first attempt got a 503 (it may have been applied) and
    /// whose retry got a 401 is sent once more with a fresh token. When that
    /// finds nothing, the error must still say the earlier attempt may have
    /// deleted it, and count every attempt: the refresh used to start the
    /// retry history afresh.
    #[tokio::test]
    async fn retry_history_survives_a_401_refresh() {
        struct Provider;
        #[async_trait::async_trait]
        impl AccessTokenProvider for Provider {
            async fn refresh_access_token(&self) -> anyhow::Result<String> {
                Ok("fresh-token".to_string())
            }
        }
        let server = MockServer::start().await;
        for status in [503, 401] {
            Mock::given(method("DELETE"))
                .respond_with(ResponseTemplate::new(status))
                .up_to_n_times(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let t = Transport::new(
            &Credentials::Refreshable {
                token: "stale-token".to_string(),
                provider: Arc::new(Provider),
            },
            Transport::for_test(&server.uri()).retry,
            EndpointPolicy::with_override(&server.uri()).unwrap(),
            None,
        )
        .unwrap();
        let sent = t
            .send(
                Method::DELETE,
                &format!("{}/v1/x", server.uri()),
                Idempotency::FromMethod,
                |rb| rb,
            )
            .await
            .unwrap();
        assert_eq!(sent.response.status(), StatusCode::NOT_FOUND);
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        assert_eq!(sent.attempts, 3);
        let note = sent.retry_note().unwrap_or_default();
        assert!(note.contains("may already have deleted it"), "{note:?}");
        assert!(note.contains("attempt 3"), "{note:?}");
    }

    /// The 401 refresh is logged at debug (which file logs always record)
    /// without the query string, such as a resumable upload's `upload_id`.
    #[tokio::test]
    async fn refresh_after_401_log_line_omits_the_query_string() {
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        struct Provider;
        #[async_trait::async_trait]
        impl AccessTokenProvider for Provider {
            async fn refresh_access_token(&self) -> anyhow::Result<String> {
                Ok("ya29.FAKE-fresh-token".to_string())
            }
        }

        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let t = Transport::new(
            &Credentials::Refreshable {
                token: "ya29.FAKE-stale-token".to_string(),
                provider: Arc::new(Provider),
            },
            Transport::for_test(&server.uri()).retry,
            EndpointPolicy::with_override(&server.uri()).unwrap(),
            None,
        )
        .unwrap();

        // Installed process-wide, not per thread: a thread-scoped default
        // races with tests on other threads registering the same callsites,
        // which can cache them as disabled for this thread too. Other tests'
        // lines land in the buffer as well; the assertions below look only
        // for this test's own URL.
        static CAPTURE: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
        let capture = CAPTURE.get_or_init(|| {
            let capture = Capture::default();
            let writer = capture.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_writer(move || writer.clone())
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("no other test installs a global subscriber");
            capture
        });
        let url = format!(
            "{}/upload/drive/v3/files?uploadType=resumable&upload_id=FAKE-upload-id",
            server.uri()
        );
        let sent = t
            .send(Method::PUT, &url, Idempotency::Idempotent, |rb| rb)
            .await
            .unwrap();
        assert_eq!(sent.response.status(), StatusCode::UNAUTHORIZED);

        let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("HTTP 401; refreshing"), "{logs}");
        assert!(logs.contains("/upload/drive/v3/files"), "{logs}");
        assert!(!logs.contains("FAKE-upload-id"), "{logs}");
    }

    /// Every `#[tokio::test]` runs on its own runtime, and a pooled
    /// connection's I/O task lives on the runtime that opened it. If test
    /// transports shared one pool, a later test could check out a connection
    /// whose task is parked on an idle runtime (the request hangs) or on one
    /// that has shut down (`runtime dropped the dispatch task`).
    #[test]
    fn test_transports_do_not_share_connections_across_runtimes() {
        let runtime = || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        };
        let first = runtime();
        let server = first.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .mount(&server)
                .await;
            server
        });
        let url = format!("{}/v1/x", server.uri());
        first
            .block_on(Transport::for_test(&server.uri()).get_json(&url, &[], "first"))
            .unwrap();
        // `first` stays alive but is never driven again, like the runtime of
        // a test that has finished, so its connection task never runs again.
        let second = runtime();
        let got = second.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                Transport::for_test(&server.uri()).get_json(&url, &[], "second"),
            )
            .await
        });
        assert!(
            matches!(got, Ok(Ok(_))),
            "second runtime reused the first runtime's connection: {got:?}"
        );
        drop(first);
    }
}
