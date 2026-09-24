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

//! The OAuth 2.0 authorization-code flow for installed apps (RFC 8252).
//!
//! * PKCE with S256 and a random `state` (via the `oauth2` crate)
//! * redirect to `http://127.0.0.1:<port>/`, the literal loopback address
//! * the loopback listener accepts only `GET /` with the expected `state`;
//!   other paths get 404, a wrong/missing state gets 400, and it keeps waiting
//!   until the right callback arrives or the timeout expires
//! * `error=` callbacks (e.g. `access_denied`) end the flow with that error
//! * a manual mode for SSH/containers: open the URL anywhere, then paste the
//!   URL the browser was redirected to (it fails to load; that is expected)
//! * `include_granted_scopes=true` for incremental authorization

use std::time::Duration;

use oauth2::url::Url;
use oauth2::{
    AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::AuthError;
use super::client_config::ClientConfig;
use super::http::{self, Endpoints};

/// Path of the loopback redirect URI.
pub const CALLBACK_PATH: &str = "/";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const PER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// How the authorization response reaches us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectMode {
    /// Local HTTP listener on 127.0.0.1.
    Loopback,
    /// The user pastes the redirected URL.
    Manual,
}

/// Options for [`run`].
#[derive(Debug, Clone)]
pub struct LoginOptions {
    pub scopes: Vec<String>,
    pub mode: RedirectMode,
    pub open_browser: bool,
    pub timeout: Duration,
    pub login_hint: Option<String>,
}

/// Result of a successful login.
#[derive(Debug)]
pub struct LoginTokens {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    /// Scopes Google reports as granted (includes earlier grants).
    pub granted_scopes: Vec<String>,
}

/// A prepared authorization request.
pub struct AuthRequest {
    pub url: Url,
    pub state: CsrfToken,
    pub verifier: PkceCodeVerifier,
}

/// Build the authorization URL with PKCE, state and incremental auth.
///
/// # Errors
///
/// Invalid endpoint or redirect URLs.
pub fn authorization_request(
    client: &ClientConfig,
    endpoints: &Endpoints,
    redirect_uri: &str,
    scopes: &[String],
    login_hint: Option<&str>,
) -> Result<AuthRequest, AuthError> {
    let oauth = super::token::oauth_client(&client.client_id, &client.client_secret, endpoints)?
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_string())
                .map_err(|e| AuthError::Config(format!("invalid redirect URI: {e}")))?,
        );
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut req = oauth
        .authorize_url(CsrfToken::new_random)
        .add_scopes(scopes.iter().cloned().map(Scope::new))
        .set_pkce_challenge(challenge)
        .add_extra_param("access_type", "offline")
        .add_extra_param("include_granted_scopes", "true")
        .add_extra_param("prompt", "consent select_account");
    if let Some(hint) = login_hint {
        req = req.add_extra_param("login_hint", hint);
    }
    let (url, state) = req.url();
    Ok(AuthRequest {
        url,
        state,
        verifier,
    })
}

/// Outcome of inspecting one callback request target.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    /// The authorization code (percent-decoded).
    Code(String),
    /// Google reported an error for our request (state matched).
    Denied {
        error: String,
        description: Option<String>,
    },
    /// Not the callback path (e.g. `/favicon.ico`).
    WrongPath,
    /// Missing or mismatched `state`: possibly forged; ignored.
    BadState,
    /// Callback path with the right state but neither `code` nor `error`.
    Malformed,
}

/// Parse a request target (`/?state=..&code=..`) or a full pasted URL.
pub fn parse_callback(target: &str, expected_state: &str) -> Callback {
    let base = Url::parse("http://127.0.0.1/").ok();
    let parsed = if target.starts_with("http://") || target.starts_with("https://") {
        Url::parse(target).ok()
    } else if target.starts_with('?') {
        base.and_then(|b| b.join(&format!("{CALLBACK_PATH}{target}")).ok())
    } else {
        base.and_then(|b| b.join(target).ok())
    };
    let Some(url) = parsed else {
        return Callback::WrongPath;
    };
    if url.path() != CALLBACK_PATH {
        return Callback::WrongPath;
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    let mut description = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "state" => state = Some(v.into_owned()),
            "code" => code = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            "error_description" => description = Some(v.into_owned()),
            _ => {}
        }
    }
    let state_ok = state.as_deref().is_some_and(|s| {
        // Constant-time comparison of the CSRF token.
        s.len() == expected_state.len()
            && s.bytes()
                .zip(expected_state.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    });
    if !state_ok {
        return Callback::BadState;
    }
    match (code, error) {
        (_, Some(error)) => Callback::Denied { error, description },
        (Some(code), None) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Malformed,
    }
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>gwsr</title></head><body \
         style=\"font-family:sans-serif;margin:3em\"><h1>{title}</h1><p>{message}</p></body></html>"
    )
}

/// Read the request line of one HTTP request (bounded size and time).
async fn read_request_target(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let read = async {
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(2).any(|w| w == b"\r\n") || buf.len() >= MAX_REQUEST_BYTES {
                break;
            }
        }
        Some(())
    };
    tokio::time::timeout(PER_CONNECTION_TIMEOUT, read)
        .await
        .ok()??;
    let line_end = buf.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&buf[..line_end]).ok()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    if method != "GET" {
        return None;
    }
    Some(target.to_string())
}

/// Serve the loopback redirect until a valid callback arrives.
///
/// # Errors
///
/// [`AuthError::Denied`] with the provider's error for denied consent, or an
/// accept error. Timeouts are applied by the caller.
pub async fn serve_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, AuthError> {
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .map_err(|e| AuthError::Network(format!("loopback listener failed: {e}")))?;
        let Some(target) = read_request_target(&mut stream).await else {
            respond(
                &mut stream,
                "400 Bad Request",
                "Bad request",
                "Unsupported request.",
            )
            .await;
            continue;
        };
        let outcome = parse_callback(&target, expected_state);
        match outcome {
            Callback::Code(code) => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Signed in",
                    "gwsr received the authorization. You can close this window.",
                )
                .await;
                return Ok(code);
            }
            Callback::Denied { error, description } => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Sign-in was not completed",
                    "gwsr did not receive an authorization. You can close this window.",
                )
                .await;
                return Err(denied(&error, description.as_deref()));
            }
            Callback::WrongPath => {
                respond(&mut stream, "404 Not Found", "Not found", "Not found.").await;
            }
            Callback::BadState | Callback::Malformed => {
                tracing::warn!(
                    "ignored an OAuth callback from {peer} with a missing or wrong \
                     state parameter"
                );
                respond(
                    &mut stream,
                    "400 Bad Request",
                    "Invalid request",
                    "This sign-in response does not belong to the running gwsr login.",
                )
                .await;
            }
        }
    }
}

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, title: &str, message: &str) {
    let response = http_response(status, &page(title, message));
    // The browser page is a courtesy; the flow result never depends on it.
    if let Err(e) = stream.write_all(response.as_bytes()).await {
        tracing::debug!(error = %e, "could not write loopback response");
    }
    if let Err(e) = stream.shutdown().await {
        tracing::debug!(error = %e, "could not close loopback connection");
    }
}

fn denied(error: &str, description: Option<&str>) -> AuthError {
    let detail = description.map(|d| format!(": {d}")).unwrap_or_default();
    let hint = match error {
        "access_denied" => " (consent was declined in the browser)",
        "invalid_scope" => " (a requested scope is unknown or not allowed for this client)",
        "admin_policy_enforced" => " (your Workspace admin blocks this app or scope)",
        _ => "",
    };
    AuthError::Denied(format!("authorization failed: {error}{detail}{hint}"))
}

/// Parse what the user pasted in manual mode (full URL or query string).
///
/// # Errors
///
/// Mismatched state, provider errors, or unrecognized input.
pub fn parse_pasted(input: &str, expected_state: &str) -> Result<String, AuthError> {
    let trimmed = input.trim();
    let target = trimmed
        .strip_prefix("http://127.0.0.1")
        .map(|rest| rest.trim_start_matches(|c: char| c == ':' || c.is_ascii_digit()))
        .unwrap_or(trimmed);
    match parse_callback(target, expected_state) {
        Callback::Code(code) => Ok(code),
        Callback::Denied { error, description } => Err(denied(&error, description.as_deref())),
        Callback::BadState => Err(AuthError::Input(
            "the pasted URL's state does not match this login attempt; paste the URL from the \
             browser tab opened for this run"
                .to_string(),
        )),
        Callback::WrongPath | Callback::Malformed => Err(AuthError::Input(
            "could not find an authorization code in the pasted text; paste the full URL from \
             the browser's address bar (it starts with http://127.0.0.1)"
                .to_string(),
        )),
    }
}

/// Exchange an authorization code (with the PKCE verifier) for tokens.
///
/// # Errors
///
/// Endpoint errors, or a response without a refresh token.
pub async fn exchange_code(
    client: &ClientConfig,
    endpoints: &Endpoints,
    redirect_uri: &str,
    code: &str,
    verifier: PkceCodeVerifier,
    requested: &[String],
) -> Result<LoginTokens, AuthError> {
    let oauth = super::token::oauth_client(&client.client_id, &client.client_secret, endpoints)?
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_string())
                .map_err(|e| AuthError::Config(format!("invalid redirect URI: {e}")))?,
        );
    let http_client = http::client()
        .map_err(|e| AuthError::Internal(format!("cannot build the HTTP client: {e:#}")))?;
    let response = oauth
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(verifier)
        .request_async(&|req| http::execute(http_client.clone(), req))
        .await
        .map_err(|e| super::token::map_token_error(e, "gwsr auth login", requested))?;
    let refresh_token = response.refresh_token().ok_or_else(|| {
        AuthError::TokenEndpoint(
            "Google returned no refresh token. Revoke gwsr's access at \
             https://myaccount.google.com/permissions and run `gwsr auth login` again"
                .to_string(),
        )
    })?;
    let granted_scopes = match response.scopes() {
        Some(s) => s.iter().map(|x| x.as_str().to_string()).collect(),
        None => {
            tracing::warn!("Google did not report the granted scopes; assuming the requested ones");
            requested.to_vec()
        }
    };
    Ok(LoginTokens {
        access_token: SecretString::from(response.access_token().secret().to_string()),
        refresh_token: SecretString::from(refresh_token.secret().to_string()),
        granted_scopes,
    })
}

fn try_open_browser(url: &str) {
    match webbrowser::open(url) {
        Ok(()) => crate::output::eprint_line("Opened your browser to sign in."),
        Err(e) => tracing::warn!("could not open a browser ({e}); open the URL above manually"),
    }
}

async fn read_line_from_stdin() -> Result<String, AuthError> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map(|_| line)
            .map_err(|e| AuthError::Input(format!("cannot read from stdin: {e}")))
    })
    .await
    .map_err(|e| AuthError::Internal(format!("stdin task failed: {e}")))?
}

/// Run the whole interactive login.
///
/// Human-readable prompts go to stderr. `on_url` is called once with the
/// authorization URL (so callers can also emit it as machine-readable output).
///
/// # Errors
///
/// Timeout, denied consent, network and endpoint errors.
pub async fn run(
    client: &ClientConfig,
    endpoints: &Endpoints,
    opts: &LoginOptions,
    on_url: &dyn Fn(&str) -> Result<(), AuthError>,
) -> Result<LoginTokens, AuthError> {
    let timeout_err = || {
        AuthError::Config(format!(
            "timed out after {}s waiting for the browser sign-in (use --timeout to wait longer)",
            opts.timeout.as_secs()
        ))
    };
    match opts.mode {
        RedirectMode::Loopback => {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| {
                AuthError::Network(format!("cannot start the local callback listener: {e}"))
            })?;
            let port = listener
                .local_addr()
                .map_err(|e| AuthError::Network(format!("cannot read the listener address: {e}")))?
                .port();
            let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
            let req = authorization_request(
                client,
                endpoints,
                &redirect,
                &opts.scopes,
                opts.login_hint.as_deref(),
            )?;
            crate::output::eprint_line(&format!(
                "Open this URL in a browser to sign in:\n\n  {}\n",
                req.url
            ));
            on_url(req.url.as_str())?;
            if opts.open_browser {
                try_open_browser(req.url.as_str());
            }
            crate::output::eprint_line(&format!(
                "Waiting for the sign-in to complete (timeout {}s)...",
                opts.timeout.as_secs()
            ));
            let code =
                tokio::time::timeout(opts.timeout, serve_callback(listener, req.state.secret()))
                    .await
                    .map_err(|_| timeout_err())??;
            exchange_code(
                client,
                endpoints,
                &redirect,
                &code,
                req.verifier,
                &opts.scopes,
            )
            .await
        }
        RedirectMode::Manual => {
            // Nothing listens on this port here; the browser (on any machine)
            // fails to load the redirect, and the user copies the URL.
            let port = 1024 + (rand::random::<u16>() % (65535 - 1024));
            let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
            let req = authorization_request(
                client,
                endpoints,
                &redirect,
                &opts.scopes,
                opts.login_hint.as_deref(),
            )?;
            crate::output::eprint_line(&format!(
                "Open this URL in a browser on any machine and sign in:\n\n  {}\n\n\
                 After approving, the browser is redirected to http://127.0.0.1:{port}/... and \
                 shows a connection error. That is expected: copy the full URL from the address \
                 bar and paste it here.",
                req.url
            ));
            on_url(req.url.as_str())?;
            crate::output::eprint_text("Redirected URL: ");
            let line = tokio::time::timeout(opts.timeout, read_line_from_stdin())
                .await
                .map_err(|_| timeout_err())??;
            let code = parse_pasted(&line, req.state.secret())?;
            exchange_code(
                client,
                endpoints,
                &redirect,
                &code,
                req.verifier,
                &opts.scopes,
            )
            .await
        }
    }
}

/// Look up the signed-in account's email address.
///
/// # Errors
///
/// Network errors or a non-success response.
pub async fn fetch_email(
    access_token: &SecretString,
    endpoints: &Endpoints,
) -> anyhow::Result<String> {
    let client = http::client()?;
    let resp = client
        .get(&endpoints.userinfo_url)
        .bearer_auth(access_token.expose_secret())
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("userinfo returned HTTP {status}");
    }
    let body: serde_json::Value = resp.json().await?;
    body.get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("userinfo response has no email"))
}

#[cfg(test)]
mod tests {
    use super::super::client_config::ClientSource;
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> ClientConfig {
        ClientConfig {
            client_id: "cid.apps.googleusercontent.com".into(),
            client_secret: SecretString::from("test-client-secret".to_string()),
            project_id: None,
            source: ClientSource::BuiltIn,
        }
    }

    #[test]
    fn authorization_url_has_pkce_state_and_incremental_auth() {
        let req = authorization_request(
            &client(),
            &Endpoints::default(),
            "http://127.0.0.1:5555/",
            &["https://www.googleapis.com/auth/drive.readonly".into()],
            Some("me@example.com"),
        )
        .unwrap();
        let pairs: std::collections::HashMap<String, String> =
            req.url.query_pairs().into_owned().collect();
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["code_challenge"].len(), 43);
        assert_eq!(pairs["state"], *req.state.secret());
        assert!(req.state.secret().len() >= 16);
        assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:5555/");
        assert_eq!(pairs["access_type"], "offline");
        assert_eq!(pairs["include_granted_scopes"], "true");
        assert_eq!(pairs["login_hint"], "me@example.com");
        assert_eq!(pairs["response_type"], "code");
        assert_eq!(
            pairs["scope"],
            "https://www.googleapis.com/auth/drive.readonly"
        );
        // The secret is long and contains '-', so random PKCE/state values
        // (base64url) cannot contain it by chance.
        assert!(!pairs.contains_key("client_secret"));
        assert!(
            !req.url.as_str().contains("test-client-secret"),
            "client secret must not be in the URL"
        );
    }

    #[test]
    fn two_requests_have_distinct_state_and_verifier() {
        let mk = || {
            authorization_request(
                &client(),
                &Endpoints::default(),
                "http://127.0.0.1:1/",
                &[],
                None,
            )
            .unwrap()
        };
        let (a, b) = (mk(), mk());
        assert_ne!(a.state.secret(), b.state.secret());
        assert_ne!(a.verifier.secret(), b.verifier.secret());
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(
            parse_callback("/?state=abc&code=4%2F0AbC%2Bx", "abc"),
            Callback::Code("4/0AbC+x".into()),
            "code must be percent-decoded"
        );
        assert_eq!(parse_callback("/favicon.ico", "abc"), Callback::WrongPath);
        assert_eq!(
            parse_callback("/other?state=abc&code=x", "abc"),
            Callback::WrongPath
        );
        assert_eq!(parse_callback("/?code=x", "abc"), Callback::BadState);
        assert_eq!(
            parse_callback("/?state=abd&code=x", "abc"),
            Callback::BadState
        );
        assert_eq!(
            parse_callback("/?state=ab&code=x", "abc"),
            Callback::BadState
        );
        assert_eq!(
            parse_callback("/?state=abc&error=access_denied", "abc"),
            Callback::Denied {
                error: "access_denied".into(),
                description: None
            }
        );
        assert_eq!(
            parse_callback("/?error=access_denied", "abc"),
            Callback::BadState
        );
        assert_eq!(parse_callback("/?state=abc", "abc"), Callback::Malformed);
    }

    #[test]
    fn pasted_input_forms() {
        assert_eq!(
            parse_pasted(
                "  http://127.0.0.1:4567/?state=s1&code=c%2F1&scope=x \n",
                "s1"
            )
            .unwrap(),
            "c/1"
        );
        assert_eq!(parse_pasted("?state=s1&code=c1", "s1").unwrap(), "c1");
        assert!(parse_pasted("http://127.0.0.1:4567/?state=zz&code=c1", "s1").is_err());
        assert!(parse_pasted("hello", "s1").is_err());
        let err = parse_pasted("http://127.0.0.1:1/?state=s1&error=access_denied", "s1")
            .unwrap_err()
            .to_string();
        assert!(err.contains("declined"), "{err}");
    }

    async fn send(port: u16, request: &str) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn loopback_ignores_wrong_path_and_state_then_accepts() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { serve_callback(listener, "st4te").await });

        let r = send(port, "GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 404"), "{r}");
        let r = send(port, "GET /?state=evil&code=attacker HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 400"), "{r}");
        let r = send(port, "POST /?state=st4te&code=x HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 400"), "{r}");
        let r = send(port, "GET /?state=st4te&code=4%2Fgood HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");

        assert_eq!(server.await.unwrap().unwrap(), "4/good");
    }

    #[tokio::test]
    async fn loopback_reports_denied_consent() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { serve_callback(listener, "s").await });
        send(port, "GET /?state=s&error=access_denied HTTP/1.1\r\n\r\n").await;
        let err = server.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("access_denied"), "{err}");
    }

    #[tokio::test]
    async fn exchange_sends_verifier_and_returns_granted_scopes() {
        let server = MockServer::start().await;
        let verifier = PkceCodeVerifier::new("v".repeat(50));
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=4%2Fabc"))
            .and(body_string_contains(format!("code_verifier={}", "v".repeat(50))))
            .and(body_string_contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2F"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.at",
                "refresh_token": "1//rt",
                "expires_in": 3599,
                "token_type": "Bearer",
                "scope": "openid https://www.googleapis.com/auth/drive.readonly https://www.googleapis.com/auth/gmail.modify"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let tokens = exchange_code(
            &client(),
            &super::super::token::tests::endpoints(&server),
            "http://127.0.0.1:9/",
            "4/abc",
            verifier,
            &["https://www.googleapis.com/auth/drive.readonly".into()],
        )
        .await
        .unwrap();
        assert_eq!(tokens.refresh_token.expose_secret(), "1//rt");
        assert_eq!(tokens.granted_scopes.len(), 3, "includes earlier grants");
    }

    #[tokio::test]
    async fn exchange_without_refresh_token_fails() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.at", "expires_in": 1, "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        let err = exchange_code(
            &client(),
            &super::super::token::tests::endpoints(&server),
            "http://127.0.0.1:9/",
            "c",
            PkceCodeVerifier::new("v".repeat(50)),
            &[],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no refresh token"), "{err}");
    }

    #[tokio::test]
    async fn userinfo_email() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"email": "a@b.c"})),
            )
            .mount(&server)
            .await;
        let email = fetch_email(
            &SecretString::from("t".to_string()),
            &super::super::token::tests::endpoints(&server),
        )
        .await
        .unwrap();
        assert_eq!(email, "a@b.c");
    }
}
