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

//! Authentication and credential management.
//!
//! * [`credentials`]: credential sources and their priority
//! * [`keystore`]: encryption at rest and data-key storage
//! * [`token`] / [`token_cache`]: access-token acquisition and caching
//! * [`flow`]: the interactive OAuth login (PKCE, loopback or manual)
//! * [`scopes`]: scope presets and per-method scope selection
//! * [`profiles`]: config directory layout and named profiles
//! * [`commands`]: the `gwsr auth ...` subcommands
//! * [`setup`]: `gwsr auth setup` (GCP project bootstrap via gcloud)

pub mod client_config;
pub mod commands;
pub mod credentials;
pub mod flow;
pub mod hardening;
pub mod http;
pub mod keystore;
pub mod profiles;
pub mod scopes;
pub mod setup;
pub mod token;
pub mod token_cache;

use secrecy::{ExposeSecret, SecretString};

use credentials::{AuthEnv, Credential, CredentialSource, Resolved};
pub use profiles::{apply_global_flags, config_dir, try_config_dir};

/// Authentication failures, classified so callers can print precise hints.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// No credential source is configured at all.
    #[error(
        "No credentials found. Run `gwsr auth login` (after `gwsr auth setup` or with an OAuth \
         client configured), or set GWSR_CREDENTIALS_FILE / GOOGLE_APPLICATION_CREDENTIALS, or \
         run `gcloud auth application-default login`"
    )]
    NoCredentials,
    /// An explicitly selected profile has no credentials.
    #[error(
        "profile '{profile}' has no credentials. Run `gwsr auth login --profile {profile}`, or \
         pick another profile with `gwsr auth use <name>` (see `gwsr auth list`)"
    )]
    ProfileNotLoggedIn { profile: String },
    /// Invalid or conflicting configuration.
    #[error("{0}")]
    Config(String),
    /// Encryption key or ciphertext problem. Files are never modified.
    #[error(transparent)]
    Keystore(#[from] keystore::KeystoreError),
    /// Reading or writing credential storage failed.
    #[error("{0}")]
    Storage(String),
    /// The refresh token is expired or revoked.
    #[error(
        "the stored refresh token was rejected ({description}); it has expired or been revoked. \
         Run `{login_command}` to sign in again"
    )]
    InvalidGrant {
        description: String,
        login_command: String,
    },
    /// Google refused the requested scopes.
    #[error(
        "Google refused the requested scopes [{}]: {description}. For a user login, add them \
         with `{}`; for a service account, ask a Workspace admin to authorize them for \
         domain-wide delegation",
        requested.join(", "),
        scopes::login_command_hint(requested)
    )]
    InvalidScope {
        requested: Vec<String>,
        description: String,
    },
    /// The token endpoint returned an error.
    #[error("token request failed: {0}")]
    TokenEndpoint(String),
    /// Network failure talking to Google's OAuth endpoints.
    #[error("{0}")]
    Network(String),
}

/// Fetches access tokens for a fixed set of scopes.
///
/// Long-running helpers use this trait so they can request a fresh token before
/// each API call instead of holding a single token string until it expires.
#[async_trait::async_trait]
pub trait AccessTokenProvider: Send + Sync {
    async fn access_token(&self) -> anyhow::Result<String>;
}

/// A token provider backed by [`get_token`].
#[derive(Debug, Clone)]
pub struct ScopedTokenProvider {
    scopes: Vec<String>,
}

impl ScopedTokenProvider {
    pub fn new(scopes: &[&str]) -> Self {
        Self {
            scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        }
    }
}

#[async_trait::async_trait]
impl AccessTokenProvider for ScopedTokenProvider {
    async fn access_token(&self) -> anyhow::Result<String> {
        let scopes: Vec<&str> = self.scopes.iter().map(String::as_str).collect();
        get_token(&scopes).await
    }
}

pub fn token_provider(scopes: &[&str]) -> ScopedTokenProvider {
    ScopedTokenProvider::new(scopes)
}

/// A fake [`AccessTokenProvider`] for tests that returns tokens from a queue.
#[cfg(test)]
pub struct FakeTokenProvider {
    tokens: std::sync::Arc<tokio::sync::Mutex<std::collections::VecDeque<String>>>,
}

#[cfg(test)]
impl FakeTokenProvider {
    pub fn new(tokens: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            tokens: std::sync::Arc::new(tokio::sync::Mutex::new(
                tokens.into_iter().map(|t| t.to_string()).collect(),
            )),
        }
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl AccessTokenProvider for FakeTokenProvider {
    async fn access_token(&self) -> anyhow::Result<String> {
        self.tokens
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no test token remaining"))
    }
}

/// Obtain an access token for `scopes` using the configured credentials
/// (see [`credentials`] for the source order).
///
/// The error's root cause is an [`AuthError`]; use
/// `err.downcast_ref::<AuthError>()` to classify it (e.g.
/// [`AuthError::NoCredentials`]).
///
/// # Errors
///
/// Any credential-resolution, storage or token-endpoint failure.
pub async fn get_token(scopes: &[&str]) -> anyhow::Result<String> {
    let env = AuthEnv::from_process()?;
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();
    let token = get_token_with(&env, &scopes).await?;
    Ok(token.expose_secret().to_string())
}

/// The `gwsr auth login` command that re-authenticates the active profile.
fn login_command(env: &AuthEnv) -> String {
    if env.profile_is_explicit() || env.profile.name != profiles::DEFAULT_PROFILE {
        format!("gwsr auth login --profile {}", env.profile.name)
    } else {
        "gwsr auth login".to_string()
    }
}

/// [`get_token`] with an explicit environment (testable, no globals).
///
/// # Errors
///
/// See [`get_token`].
pub async fn get_token_with(env: &AuthEnv, scopes: &[String]) -> Result<SecretString, AuthError> {
    let load_env = env.clone();
    let (source, resolved) = tokio::task::spawn_blocking(move || load_env.load())
        .await
        .map_err(|e| AuthError::Storage(format!("credential loading task failed: {e}")))??;

    let credential = match resolved {
        Resolved::Token(token) => return Ok(token),
        Resolved::Credential(c) => c,
    };

    match &credential {
        Credential::AuthorizedUser(user) => {
            let granted = match &source {
                CredentialSource::Profile { .. } => {
                    let path = env.paths.metadata.clone();
                    tokio::task::spawn_blocking(move || profiles::load_metadata(&path))
                        .await
                        .map_err(|e| AuthError::Storage(format!("metadata task failed: {e}")))?
                        .map_err(|e| AuthError::Storage(format!("{e:#}")))?
                        .map(|m| m.granted_scopes)
                }
                _ => None,
            };
            let downscope = downscope(scopes, granted.as_deref());
            let login = login_command(env);
            let endpoints = env.endpoints.clone();
            let fetch = || async {
                token::refresh_user(user, downscope.as_deref(), &endpoints, &login).await
            };
            if matches!(source, CredentialSource::Profile { .. }) {
                let key = format!(
                    "{}|{}",
                    credential.identity(),
                    downscope
                        .as_ref()
                        .map(|s| sorted_join(s))
                        .unwrap_or_else(|| "*".to_string())
                );
                let cache = token_cache::TokenCache::new(
                    &env.paths.token_cache,
                    &env.paths.token_lock,
                    env.keystore()?,
                );
                cache.get_or_fetch(&key, fetch).await
            } else {
                Ok(fetch().await?.token)
            }
        }
        Credential::ServiceAccount(sa) => {
            let subject = env.impersonate.as_deref();
            Ok(
                token::service_account_token(sa, scopes, subject, &env.endpoints)
                    .await?
                    .token,
            )
        }
    }
}

/// Scopes to down-scope a user token to: the requested scopes when they are
/// all known to be granted; otherwise `None` (the token carries every granted
/// scope, and the API decides).
fn downscope(requested: &[String], granted: Option<&[String]>) -> Option<Vec<String>> {
    let granted = granted.filter(|g| !g.is_empty())?;
    if requested.is_empty() || !requested.iter().all(|r| granted.contains(r)) {
        if !requested.is_empty() {
            tracing::debug!(
                ?requested,
                "requested scopes are not all in the recorded grant; requesting all granted scopes"
            );
        }
        return None;
    }
    Some(requested.to_vec())
}

fn sorted_join(scopes: &[String]) -> String {
    let mut v: Vec<&str> = scopes.iter().map(String::as_str).collect();
    v.sort_unstable();
    v.dedup();
    v.join(" ")
}

/// Scopes granted to the active profile, as recorded at login.
///
/// Makes no network calls and never touches the keyring. Returns `Ok(None)`
/// when the credentials in use do not come from a gwsr profile (token env
/// var, credentials file, ADC) or nothing was recorded.
///
/// # Errors
///
/// Fails when the profile metadata exists but cannot be read.
pub fn granted_scopes() -> anyhow::Result<Option<Vec<String>>> {
    let env = AuthEnv::from_process()?;
    granted_scopes_for(&env)
}

fn granted_scopes_for(env: &AuthEnv) -> anyhow::Result<Option<Vec<String>>> {
    match env.source() {
        Ok(CredentialSource::Profile { .. }) => Ok(profiles::load_metadata(&env.paths.metadata)?
            .map(|m| m.granted_scopes)
            .filter(|s| !s.is_empty())),
        // Other sources have no recorded grant. Resolution errors are not
        // hidden: get_token() reports the same error on the next call.
        _ => Ok(None),
    }
}

/// Choose the scope for a Discovery method call, warning loudly on stderr
/// when the recorded grant does not cover the method.
///
/// # Errors
///
/// Fails only if recorded profile metadata is unreadable.
pub fn scopes_for_method(
    method_scopes: &[String],
    http_method: &str,
) -> anyhow::Result<Vec<String>> {
    let granted = granted_scopes()?;
    let choice = scopes::select_for_method(method_scopes, http_method, granted.as_deref());
    if let scopes::ScopeChoice::NotGranted {
        required_any_of, ..
    } = &choice
    {
        eprintln!(
            "warning: this method needs one of [{}], but the active profile was granted none of \
             them. Add one with `{}`",
            required_any_of.join(", "),
            scopes::login_command_hint(&required_any_of[..1])
        );
    }
    Ok(choice.scopes().into_iter().map(str::to_string).collect())
}

/// Returns the project ID used for quota and billing (`x-goog-user-project`).
///
/// Priority: `GWSR_PROJECT_ID`, then the OAuth client configuration's project,
/// then `quota_project_id` from Application Default Credentials.
pub fn get_quota_project() -> Option<String> {
    match profiles::env_string("GWSR_PROJECT_ID") {
        Ok(Some(project_id)) => return Some(project_id),
        Ok(None) => {}
        Err(e) => eprintln!("warning: ignoring GWSR_PROJECT_ID: {e:#}"),
    }
    match client_config::load_saved() {
        Ok(Some(config)) => {
            if let Some(project) = config.project_id.filter(|p| !p.is_empty()) {
                return Some(project);
            }
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("warning: ignoring the OAuth client config for the quota project: {e:#}")
        }
    }
    let path = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            dirs::home_dir().map(|h| {
                h.join(".config")
                    .join("gcloud")
                    .join("application_default_credentials.json")
            })
        })?;
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "warning: cannot read '{}' for the quota project: {e}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(json) => json
            .get("quota_project_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        Err(e) => {
            eprintln!(
                "warning: '{}' is not valid JSON; ignoring it for the quota project: {e}",
                path.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keystore::Purpose;
    use super::keystore::testing::memory_keystore;
    use super::profiles::{ActiveProfile, ProfileMetadata, ProfilePaths, ProfileSource};
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const USER_JSON: &str =
        r#"{"type":"authorized_user","client_id":"cid","client_secret":"cs","refresh_token":"rt"}"#;

    fn profile_env(dir: &std::path::Path, server: &MockServer) -> AuthEnv {
        let (ks, _) = memory_keystore(dir);
        let paths = ProfilePaths::new(dir, "default");
        paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(Purpose::Credentials, USER_JSON.as_bytes())
            .unwrap();
        crate::fs_util::atomic_write(&paths.credentials, &ct).unwrap();
        let mut env = AuthEnv::new(
            dir.to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            paths,
        )
        .with_keystore(ks);
        env.endpoints = token::tests::endpoints(server);
        env
    }

    #[test]
    fn downscope_rules() {
        let g = vec!["a".to_string(), "b".to_string()];
        assert_eq!(downscope(&["a".into()], Some(&g)), Some(vec!["a".into()]));
        assert_eq!(downscope(&["c".into()], Some(&g)), None);
        assert_eq!(downscope(&["a".into()], None), None);
        assert_eq!(downscope(&[], Some(&g)), None);
    }

    #[tokio::test]
    async fn profile_token_is_downscoped_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("scope=gmail.readonly"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.ro", "expires_in": 3600, "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let env = profile_env(dir.path(), &server);
        profiles::save_metadata(
            &env.paths.metadata,
            &ProfileMetadata {
                account: None,
                client_id: "cid".into(),
                granted_scopes: vec!["gmail.readonly".into(), "openid".into()],
                updated_at: "t".into(),
            },
        )
        .unwrap();
        let scopes = vec!["gmail.readonly".to_string()];
        for _ in 0..2 {
            let t = get_token_with(&env, &scopes).await.unwrap();
            assert_eq!(t.expose_secret(), "ya29.ro");
        }
        assert!(env.paths.token_cache.exists());
        assert_eq!(
            granted_scopes_for(&env).unwrap(),
            Some(vec!["gmail.readonly".into(), "openid".into()])
        );
    }

    #[tokio::test]
    async fn expired_refresh_token_suggests_login() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let env = profile_env(dir.path(), &server);
        let err = get_token_with(&env, &["x".to_string()]).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidGrant { .. }));
        assert!(err.to_string().contains("gwsr auth login"));
        assert!(
            env.paths.credentials.exists(),
            "credentials survive invalid_grant"
        );
    }

    #[tokio::test]
    async fn no_credentials_is_classifiable_through_anyhow() {
        let dir = tempfile::tempdir().unwrap();
        let env = AuthEnv::new(
            dir.path().to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(dir.path(), "default"),
        );
        let err: anyhow::Error = get_token_with(&env, &[]).await.unwrap_err().into();
        assert!(matches!(
            err.downcast_ref::<AuthError>(),
            Some(AuthError::NoCredentials)
        ));
    }

    #[tokio::test]
    async fn env_token_short_circuits() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = AuthEnv::new(
            dir.path().to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(dir.path(), "default"),
        );
        env.token = Some(SecretString::from("tok".to_string()));
        assert_eq!(
            get_token_with(&env, &[]).await.unwrap().expose_secret(),
            "tok"
        );
        assert_eq!(granted_scopes_for(&env).unwrap(), None);
    }

    #[test]
    fn login_command_mentions_non_default_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |name: &str, source| {
            AuthEnv::new(
                dir.path().to_path_buf(),
                ActiveProfile {
                    name: name.into(),
                    source,
                },
                ProfilePaths::new(dir.path(), name),
            )
        };
        assert_eq!(
            login_command(&mk("default", ProfileSource::Default)),
            "gwsr auth login"
        );
        assert_eq!(
            login_command(&mk("work", ProfileSource::ActiveProfileFile)),
            "gwsr auth login --profile work"
        );
    }
}
