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

//! The credential-attaching request path.
//!
//! [`Transport::send`] is the only function in the executor that puts a bearer
//! token on a request. It validates the destination with
//! [`EndpointPolicy::check`] first, sends through the shared retry policy,
//! and on a 401 refreshes the token once through the
//! [`AccessTokenProvider`](crate::auth::AccessTokenProvider).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Method, StatusCode};

use gws_rust_core::client::{Idempotency, RetryPolicy, Sent};
use gws_rust_core::validate::EndpointPolicy;

use super::AuthMethod;
use crate::auth::AccessTokenProvider;
use crate::error::GwsError;

/// How requests are authenticated.
#[derive(Clone)]
pub enum Credentials {
    /// Unauthenticated requests.
    None,
    /// A fixed access token that cannot be refreshed.
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
            _ => AuthMethod::OAuth,
        }
    }
}

pub(crate) struct Transport {
    client: reqwest::Client,
    pub(crate) retry: RetryPolicy,
    endpoints: EndpointPolicy,
    token: Mutex<Option<String>>,
    provider: Option<Arc<dyn AccessTokenProvider>>,
    quota_project: Option<String>,
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
            client: crate::client::shared_client()?,
            retry,
            endpoints,
            token: Mutex::new(token),
            provider,
            quota_project,
        })
    }

    pub(crate) fn endpoints(&self) -> &EndpointPolicy {
        &self.endpoints
    }

    pub(crate) fn auth_method(&self) -> Result<AuthMethod, GwsError> {
        Ok(if self.current_token()?.is_some() {
            AuthMethod::OAuth
        } else {
            AuthMethod::None
        })
    }

    fn current_token(&self) -> Result<Option<String>, GwsError> {
        self.token
            .lock()
            .map(|t| t.clone())
            .map_err(|_| GwsError::other(anyhow::anyhow!("access token lock poisoned")))
    }

    async fn refresh_token(&self) -> Result<(), GwsError> {
        let Some(provider) = &self.provider else {
            return Ok(());
        };
        let fresh = provider.access_token().await.map_err(|e| {
            GwsError::Auth(format!(
                "Failed to refresh access token after HTTP 401: {e:#}"
            ))
        })?;
        let mut guard = self
            .token
            .lock()
            .map_err(|_| GwsError::other(anyhow::anyhow!("access token lock poisoned")))?;
        *guard = Some(fresh);
        Ok(())
    }

    /// Send `method url` with the default retry policy.
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
        let url = self.endpoints.check(url)?;
        let mut refreshed = false;
        loop {
            let token = self.current_token()?;
            let sent = gws_rust_core::client::send(policy, idempotency, || {
                let mut rb = self.client.request(method.clone(), url.clone());
                if let Some(token) = &token {
                    rb = rb.bearer_auth(token);
                }
                if let Some(project) = &self.quota_project {
                    rb = rb.header("x-goog-user-project", project);
                }
                customize(rb)
            })
            .await?;
            // A 401 means the server rejected the credential before doing any
            // work, so re-sending (even a POST) cannot duplicate an effect.
            if sent.response.status() == StatusCode::UNAUTHORIZED
                && self.provider.is_some()
                && !refreshed
            {
                tracing::debug!(%url, "HTTP 401; refreshing access token and retrying once");
                refreshed = true;
                self.refresh_token().await?;
                continue;
            }
            return Ok(sent);
        }
    }
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
            .map_err(|_| {
                GwsError::other(anyhow::anyhow!(
                    "no data received for {}s while reading the response body (see --timeout)",
                    limit.as_secs()
                ))
            })?,
        None => stream.next().await,
    };
    next.transpose()
        .map_err(|e| GwsError::other(anyhow::Error::new(e).context("failed to read response body")))
}

/// Timeout for a request that uploads `bytes`: the base response timeout plus
/// one second per 64 KiB, so slow links are not cut off mid-upload.
pub(crate) fn upload_timeout(base: Option<Duration>, bytes: u64) -> Option<Duration> {
    base.map(|b| b + Duration::from_secs(bytes / (64 * 1024)))
}
