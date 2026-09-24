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

//! Access-token acquisition: refresh-token grant (via the `oauth2` crate) and
//! the service-account JWT bearer grant (RFC 7523) with optional domain-wide
//! delegation (`sub`).

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use oauth2::basic::{BasicClient, BasicErrorResponseType};
use oauth2::{
    AuthUrl, ClientId, ClientSecret, RefreshToken, RequestTokenError, Scope, TokenResponse,
    TokenUrl,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use zeroize::Zeroizing;

use super::AuthError;
use super::credentials::{AuthorizedUser, ServiceAccountKey};
use super::http::{self, Endpoints};

/// Lifetime requested for service-account assertions (Google's maximum).
const JWT_LIFETIME_SECS: i64 = 3600;

/// A freshly obtained access token.
#[derive(Debug, Clone)]
pub struct AccessToken {
    pub token: SecretString,
    /// Unix seconds; `None` if the server did not say.
    pub expires_at: Option<i64>,
    /// Scopes reported by the token endpoint, if any.
    pub scopes: Option<Vec<String>>,
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Build an `oauth2` client for Google with `client_secret` sent in the body.
pub(crate) fn oauth_client(
    client_id: &str,
    client_secret: &SecretString,
    endpoints: &Endpoints,
) -> Result<
    BasicClient<
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    AuthError,
> {
    let auth_url = AuthUrl::new(endpoints.auth_url.clone())
        .map_err(|e| AuthError::Config(format!("invalid authorization URL: {e}")))?;
    let token_url = TokenUrl::new(endpoints.token_url.clone())
        .map_err(|e| AuthError::Config(format!("invalid token URL: {e}")))?;
    Ok(BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.expose_secret().to_string()))
        .set_auth_type(oauth2::AuthType::RequestBody)
        .set_auth_uri(auth_url)
        .set_token_uri(token_url))
}

/// Convert an `oauth2` token-request error into an [`AuthError`].
pub(crate) fn map_token_error<RE: std::error::Error + 'static>(
    err: RequestTokenError<RE, oauth2::basic::BasicErrorResponse>,
    login_command: &str,
    requested: &[String],
) -> AuthError {
    match err {
        RequestTokenError::ServerResponse(resp) => {
            let description = resp
                .error_description()
                .cloned()
                .unwrap_or_else(|| resp.error().to_string());
            match resp.error() {
                BasicErrorResponseType::InvalidGrant => AuthError::InvalidGrant {
                    description,
                    login_command: login_command.to_string(),
                },
                BasicErrorResponseType::InvalidScope => AuthError::InvalidScope {
                    requested: requested.to_vec(),
                    description,
                },
                other => AuthError::TokenEndpoint(format!("{other}: {description}")),
            }
        }
        RequestTokenError::Request(e) => AuthError::Network(format!("{e}")),
        RequestTokenError::Parse(e, _) => {
            AuthError::TokenEndpoint(format!("unparseable token response: {e}"))
        }
        RequestTokenError::Other(e) => AuthError::TokenEndpoint(e),
    }
}

/// Exchange a refresh token for an access token.
///
/// When `scopes` is `Some`, the token is down-scoped to them (RFC 6749 §6);
/// they must be a subset of the granted scopes. `None` yields a token carrying
/// every granted scope.
///
/// # Errors
///
/// [`AuthError::InvalidGrant`] for an expired/revoked refresh token,
/// [`AuthError::InvalidScope`], network and endpoint errors.
pub async fn refresh_user(
    user: &AuthorizedUser,
    scopes: Option<&[String]>,
    endpoints: &Endpoints,
    login_command: &str,
) -> Result<AccessToken, AuthError> {
    let client = oauth_client(&user.client_id, &user.client_secret, endpoints)?;
    let http_client = http::client().map_err(|e| AuthError::Network(format!("{e:#}")))?;
    let refresh = RefreshToken::new(user.refresh_token.expose_secret().to_string());
    let mut request = client.exchange_refresh_token(&refresh);
    if let Some(scopes) = scopes {
        request = request.add_scopes(scopes.iter().cloned().map(Scope::new));
    }
    let requested = scopes.map(<[String]>::to_vec).unwrap_or_default();
    let response = request
        .request_async(&|req| http::execute(http_client.clone(), req))
        .await
        .map_err(|e| map_token_error(e, login_command, &requested))?;
    Ok(AccessToken {
        token: SecretString::from(response.access_token().secret().to_string()),
        // Saturating: an absurd lifetime clamps far in the future (without
        // overflowing the addition) instead of being dropped.
        expires_at: response
            .expires_in()
            .map(|d| now_unix() + i64::try_from(d.as_secs()).unwrap_or(i64::MAX / 2)),
        scopes: response
            .scopes()
            .map(|s| s.iter().map(|x| x.as_str().to_string()).collect()),
    })
}

#[derive(Deserialize)]
struct JwtTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct EndpointError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Decode a PEM private key (PKCS#8 or PKCS#1 RSA) into a signing key.
fn parse_private_key(pem: &str) -> Result<RsaKeyPair, AuthError> {
    let (label, body) = pem
        .lines()
        .map(str::trim)
        .skip_while(|l| !l.starts_with("-----BEGIN "))
        .fold(
            (None::<String>, String::new()),
            |(label, mut body), line| {
                if line.starts_with("-----BEGIN ") {
                    (Some(line.to_string()), body)
                } else if line.starts_with("-----END ") || label.is_none() {
                    (label, body)
                } else {
                    body.push_str(line);
                    (label, body)
                }
            },
        );
    let body = Zeroizing::new(body);
    let label = label.ok_or_else(|| {
        AuthError::Config("service-account private_key is not a PEM key".to_string())
    })?;
    let der = Zeroizing::new(STANDARD.decode(body.as_bytes()).map_err(|e| {
        AuthError::Config(format!("service-account private_key is not valid PEM: {e}"))
    })?);
    let key = if label.contains("RSA PRIVATE KEY") {
        RsaKeyPair::from_der(&der)
    } else {
        RsaKeyPair::from_pkcs8(&der)
    };
    key.map_err(|e| AuthError::Config(format!("service-account private_key is unusable: {e}")))
}

/// Build and sign the RFC 7523 assertion.
pub(crate) fn sign_assertion(
    sa: &ServiceAccountKey,
    scopes: &[String],
    subject: Option<&str>,
    audience: &str,
    now: i64,
) -> Result<Zeroizing<String>, AuthError> {
    let mut header = serde_json::json!({"alg": "RS256", "typ": "JWT"});
    if let Some(kid) = &sa.private_key_id {
        header["kid"] = serde_json::json!(kid);
    }
    let mut claims = serde_json::json!({
        "iss": sa.client_email,
        "scope": scopes.join(" "),
        "aud": audience,
        "iat": now,
        "exp": now + JWT_LIFETIME_SECS,
    });
    if let Some(sub) = subject {
        claims["sub"] = serde_json::json!(sub);
    }
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let key = parse_private_key(sa.private_key.expose_secret())?;
    let mut signature = vec![0u8; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|e| AuthError::Config(format!("cannot sign service-account assertion: {e}")))?;
    Ok(Zeroizing::new(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    )))
}

/// Obtain an access token for a service account, impersonating `subject`
/// (domain-wide delegation) when given.
///
/// # Errors
///
/// Key parsing, signing, network and endpoint errors. An `unauthorized_client`
/// response with a subject explains how to configure domain-wide delegation.
pub async fn service_account_token(
    sa: &ServiceAccountKey,
    scopes: &[String],
    subject: Option<&str>,
    endpoints: &Endpoints,
) -> Result<AccessToken, AuthError> {
    if scopes.is_empty() {
        return Err(AuthError::Config(
            "a service-account token needs at least one scope".to_string(),
        ));
    }
    let assertion = sign_assertion(sa, scopes, subject, &endpoints.token_url, now_unix())?;
    let http_client = http::client().map_err(|e| AuthError::Network(format!("{e:#}")))?;
    let response = http_client
        .post(&endpoints.token_url)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("service-account token request failed: {e}")))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| AuthError::Network(format!("cannot read token response: {e}")))?;
    if !status.is_success() {
        let detail = match serde_json::from_slice::<EndpointError>(&body) {
            Ok(err) => {
                let desc = err.error_description.unwrap_or_default();
                if err.error == "unauthorized_client" && subject.is_some() {
                    return Err(AuthError::TokenEndpoint(format!(
                        "unauthorized_client: {desc}. Impersonating '{}' requires domain-wide \
                         delegation: a Workspace admin must authorize the client ID {} of \
                         service account '{}' for the scopes [{}] in the Admin console \
                         (Security > Access and data control > API controls > Domain-wide \
                         delegation)",
                        subject.unwrap_or_default(),
                        sa.client_id
                            .as_deref()
                            .unwrap_or("(see the key file's client_id)"),
                        sa.client_email,
                        scopes.join(", ")
                    )));
                }
                if err.error == "invalid_scope" {
                    return Err(AuthError::InvalidScope {
                        requested: scopes.to_vec(),
                        description: desc,
                    });
                }
                format!("{}: {desc}", err.error)
            }
            Err(_) => format!(
                "HTTP {status}: {}",
                crate::output::sanitize_for_terminal(&String::from_utf8_lossy(&body))
            ),
        };
        return Err(AuthError::TokenEndpoint(detail));
    }
    let parsed: JwtTokenResponse = serde_json::from_slice(&body)
        .map_err(|e| AuthError::TokenEndpoint(format!("unparseable token response: {e}")))?;
    Ok(AccessToken {
        token: SecretString::from(parsed.access_token),
        expires_at: parsed.expires_in.map(|s| now_unix() + s),
        scopes: Some(scopes.to_vec()),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A throwaway 2048-bit RSA key in PKCS#8 PEM, generated for tests only.
    pub(crate) fn test_sa_key() -> ServiceAccountKey {
        use aws_lc_rs::encoding::AsDer;
        let pkcs8 = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
            .unwrap()
            .as_der()
            .unwrap();
        let b64 = STANDARD.encode(pkcs8.as_ref());
        let mut pem = String::from("-----BEGIN PRIVATE KEY-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END PRIVATE KEY-----\n");
        ServiceAccountKey {
            client_email: "sa@p.iam.gserviceaccount.com".into(),
            private_key: SecretString::from(pem),
            private_key_id: Some("kid1".into()),
            client_id: Some("1234567890".into()),
        }
    }

    fn user() -> AuthorizedUser {
        AuthorizedUser {
            client_id: "cid".into(),
            client_secret: SecretString::from("csecret".to_string()),
            refresh_token: SecretString::from("1//rt".to_string()),
        }
    }

    pub(crate) fn endpoints(server: &MockServer) -> Endpoints {
        Endpoints {
            auth_url: format!("{}/auth", server.uri()),
            token_url: format!("{}/token", server.uri()),
            revoke_url: format!("{}/revoke", server.uri()),
            userinfo_url: format!("{}/userinfo", server.uri()),
        }
    }

    fn decode_part(part: &str) -> serde_json::Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn assertion_contains_subject_scope_and_valid_signature() {
        let sa = test_sa_key();
        let jwt = sign_assertion(
            &sa,
            &["https://www.googleapis.com/auth/admin.directory.user.readonly".into()],
            Some("admin@example.com"),
            "https://oauth2.googleapis.com/token",
            1_000,
        )
        .unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header = decode_part(parts[0]);
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["kid"], "kid1");
        let claims = decode_part(parts[1]);
        assert_eq!(claims["iss"], "sa@p.iam.gserviceaccount.com");
        assert_eq!(claims["sub"], "admin@example.com");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["exp"], 1_000 + JWT_LIFETIME_SECS);

        // Verify with the public key.
        let key = parse_private_key(sa.private_key.expose_secret()).unwrap();
        use aws_lc_rs::signature::KeyPair;
        let public = aws_lc_rs::signature::UnparsedPublicKey::new(
            &aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256,
            key.public_key().as_ref(),
        );
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        public
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .unwrap();
    }

    #[test]
    fn assertion_without_subject_has_no_sub() {
        let jwt = sign_assertion(&test_sa_key(), &["s".into()], None, "aud", 0).unwrap();
        let claims = decode_part(jwt.split('.').nth(1).unwrap());
        assert!(claims.get("sub").is_none());
    }

    #[test]
    fn bad_pem_is_a_clear_error() {
        assert!(parse_private_key("not a key").is_err());
        assert!(
            parse_private_key("-----BEGIN PRIVATE KEY-----\n!!!\n-----END PRIVATE KEY-----")
                .is_err()
        );
    }

    #[tokio::test]
    async fn refresh_sends_scope_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=1%2F%2Frt"))
            .and(body_string_contains("client_secret=csecret"))
            .and(body_string_contains(
                "scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.readonly",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.new",
                "expires_in": 3599,
                "token_type": "Bearer",
                "scope": "https://www.googleapis.com/auth/gmail.readonly"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let scopes = vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()];
        let t = refresh_user(
            &user(),
            Some(&scopes),
            &endpoints(&server),
            "gwsr auth login",
        )
        .await
        .unwrap();
        assert_eq!(t.token.expose_secret(), "ya29.new");
        assert!(t.expires_at.unwrap() > now_unix() + 3500);
        assert_eq!(t.scopes.unwrap(), scopes);
    }

    #[tokio::test]
    async fn invalid_grant_maps_to_actionable_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let err = refresh_user(
            &user(),
            None,
            &endpoints(&server),
            "gwsr auth login --profile w",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AuthError::InvalidGrant { .. }), "{err}");
        assert!(
            err.to_string().contains("gwsr auth login --profile w"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn invalid_scope_maps_to_invalid_scope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_scope",
            })))
            .mount(&server)
            .await;
        let scopes = vec!["x".to_string()];
        let err = refresh_user(&user(), Some(&scopes), &endpoints(&server), "l")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidScope { .. }), "{err}");
    }

    #[tokio::test]
    async fn service_account_token_with_subject() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer",
            ))
            .and(body_string_contains("assertion="))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.sa",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let t = service_account_token(
            &test_sa_key(),
            &["s".into()],
            Some("u@example.com"),
            &endpoints(&server),
        )
        .await
        .unwrap();
        assert_eq!(t.token.expose_secret(), "ya29.sa");
    }

    #[tokio::test]
    async fn service_account_delegation_error_explains_setup() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "unauthorized_client",
                "error_description": "Client is unauthorized to retrieve access tokens using this method"
            })))
            .mount(&server)
            .await;
        let err = service_account_token(
            &test_sa_key(),
            &["s".into()],
            Some("u@example.com"),
            &endpoints(&server),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("domain-wide delegation"), "{err}");
    }

    #[tokio::test]
    async fn service_account_requires_scopes() {
        let err = service_account_token(&test_sa_key(), &[], None, &Endpoints::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Config(_)));
    }
}
