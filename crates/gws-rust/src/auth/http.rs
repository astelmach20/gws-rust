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

//! HTTP plumbing for OAuth endpoints.
//!
//! One `reqwest` client (proxy-aware via the standard `*_PROXY` variables)
//! serves the login flow, token refresh, revocation and userinfo. It never
//! follows redirects, as recommended for OAuth token endpoints, and has a
//! request timeout so a stalled endpoint cannot hang the CLI.

use std::sync::OnceLock;
use std::time::Duration;

/// Google OAuth endpoints. Overridable for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub auth_url: String,
    pub token_url: String,
    pub revoke_url: String,
    pub userinfo_url: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            revoke_url: "https://oauth2.googleapis.com/revoke".to_string(),
            userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".to_string(),
        }
    }
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Error from the OAuth HTTP adapter.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("invalid HTTP response: {0}")]
    Response(#[from] oauth2::http::Error),
}

/// The shared OAuth HTTP client.
///
/// # Errors
///
/// Fails if the TLS backend cannot be initialized.
pub fn client() -> anyhow::Result<reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .user_agent(concat!("gwsr/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| format!("cannot build the OAuth HTTP client: {e}"))
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

/// Execute an `oauth2` request with `client`.
///
/// # Errors
///
/// Network, timeout or response-conversion errors.
pub async fn execute(
    client: reqwest::Client,
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, HttpError> {
    let request = reqwest::Request::try_from(request)?;
    let response = client.execute(request).await?;
    let mut builder = oauth2::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        builder = builder.header(name, value);
    }
    let body = response.bytes().await?.to_vec();
    Ok(builder.body(body)?)
}
