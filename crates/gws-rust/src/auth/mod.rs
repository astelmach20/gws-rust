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
pub mod http;
pub mod keystore;
pub mod profiles;
pub mod scopes;
pub mod setup;
pub mod token;
pub mod token_cache;

use secrecy::{ExposeSecret, SecretString};

use crate::error::GwsError;

use credentials::{AuthEnv, Credential, CredentialSource, Resolved};
pub use profiles::try_config_dir;
use token_cache::Freshness;

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
    /// The token endpoint answered 429 or 5xx: a transient failure on
    /// Google's side. The credential itself was not rejected.
    #[error(
        "Google's OAuth token endpoint is temporarily unavailable (HTTP {status}): {detail}. \
         The credentials were not rejected; retry later"
    )]
    TokenEndpointUnavailable { status: u16, detail: String },
    /// Network failure talking to Google's OAuth endpoints.
    #[error("{0}")]
    Network(String),
    /// The user or a Workspace policy denied the authorization request.
    #[error("{0}")]
    Denied(String),
    /// Unusable interactive input (e.g. a pasted callback URL).
    #[error("{0}")]
    Input(String),
    /// An unexpected internal failure (a background task panicked, stdout
    /// could not be written).
    #[error("{0}")]
    Internal(String),
}

/// The CLI error category of an auth-layer failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Credential,
    Config,
    Store,
    Network,
    /// A retryable HTTP status from the token endpoint.
    Unavailable(u16),
    Input,
    Internal,
}

impl Category {
    fn of(e: &AuthError) -> Self {
        match e {
            AuthError::NoCredentials
            | AuthError::ProfileNotLoggedIn { .. }
            | AuthError::InvalidGrant { .. }
            | AuthError::InvalidScope { .. }
            | AuthError::TokenEndpoint(_)
            | AuthError::Denied(_) => Category::Credential,
            AuthError::Config(_) => Category::Config,
            AuthError::Keystore(_) => Category::Store,
            AuthError::Storage(_) => Category::Store,
            AuthError::Network(_) => Category::Network,
            AuthError::TokenEndpointUnavailable { status, .. } => Category::Unavailable(*status),
            AuthError::Input(_) => Category::Input,
            AuthError::Internal(_) => Category::Internal,
        }
    }

    fn error(self, message: String) -> GwsError {
        match self {
            Category::Credential => GwsError::Auth(message),
            Category::Config => GwsError::Config(message),
            Category::Store => GwsError::CredentialStore(message),
            Category::Network => GwsError::Network(message.into()),
            Category::Unavailable(code) => GwsError::Api {
                code,
                message,
                reason: "tokenEndpointUnavailable".to_string(),
                enable_url: None,
            },
            Category::Input => GwsError::Validation(message),
            Category::Internal => GwsError::other(message),
        }
    }
}

impl From<AuthError> for GwsError {
    fn from(e: AuthError) -> Self {
        Category::of(&e).error(e.to_string())
    }
}

impl From<keystore::KeystoreError> for GwsError {
    fn from(e: keystore::KeystoreError) -> Self {
        Category::Store.error(e.to_string())
    }
}

/// Convert an `anyhow` error from the auth layer into the CLI error type,
/// keeping the whole context chain in the message.
///
/// The category comes from the first typed cause in the chain:
/// [`AuthError`] and [`keystore::KeystoreError`] map as in their `From`
/// impls; a bare `std::io::Error` is a local storage failure
/// ([`GwsError::CredentialStore`]). Everything else the auth layer reports
/// untyped is a configuration problem (invalid environment variables,
/// profile names, config or client files) and becomes [`GwsError::Config`].
pub(crate) fn to_gws_error(err: anyhow::Error) -> GwsError {
    let category = err
        .chain()
        .find_map(|cause| {
            if let Some(e) = cause.downcast_ref::<AuthError>() {
                Some(Category::of(e))
            } else if cause.downcast_ref::<keystore::KeystoreError>().is_some()
                || cause.downcast_ref::<std::io::Error>().is_some()
            {
                Some(Category::Store)
            } else {
                None
            }
        })
        .unwrap_or(Category::Config);
    category.error(format!("{err:#}"))
}

/// Mints replacement access tokens for a fixed set of scopes.
///
/// [`crate::transport::Transport`] calls this once when an API rejects the
/// current token, so long-running commands survive token expiry.
#[async_trait::async_trait]
pub trait AccessTokenProvider: Send + Sync {
    /// A newly minted token that bypasses the cache. Called after an API
    /// rejected the current token with HTTP 401.
    async fn refresh_access_token(&self) -> anyhow::Result<String>;
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
    async fn refresh_access_token(&self) -> anyhow::Result<String> {
        let scopes: Vec<&str> = self.scopes.iter().map(String::as_str).collect();
        refresh_token(&scopes).await
    }
}

pub fn token_provider(scopes: &[&str]) -> ScopedTokenProvider {
    ScopedTokenProvider::new(scopes)
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
    token_for(scopes, Freshness::Cached).await
}

/// Like [`get_token`], but never reuses a cached token: used after an API
/// answered HTTP 401 to the cached one.
///
/// # Errors
///
/// See [`get_token`].
pub async fn refresh_token(scopes: &[&str]) -> anyhow::Result<String> {
    token_for(scopes, Freshness::ForceRefresh).await
}

async fn token_for(scopes: &[&str], freshness: Freshness) -> anyhow::Result<String> {
    let env = AuthEnv::from_process()?;
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();
    let token = get_token_fresh(&env, &scopes, freshness).await?;
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

/// [`get_token`] with an explicit environment (testable, no globals) and
/// explicit cache freshness.
///
/// # Errors
///
/// See [`get_token`].
pub async fn get_token_fresh(
    env: &AuthEnv,
    scopes: &[String],
    freshness: Freshness,
) -> Result<SecretString, AuthError> {
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
                // The grant fingerprint keeps a token minted from a previous
                // login's refresh token from ever being served for this one.
                let key = format!(
                    "{}|grant:{}|{}",
                    credential.identity(),
                    token_cache::grant_fingerprint(&user.refresh_token),
                    downscope
                        .as_ref()
                        .map(|s| sorted_join(s))
                        .unwrap_or_else(|| "*".to_string())
                );
                let cache = token_cache::TokenCache::new(
                    &env.paths.token_cache,
                    &env.paths.token_lock,
                    env.keystore(),
                );
                cache.get_or_fetch(&key, freshness, fetch).await
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

/// A stable, non-secret key for the identity requests are made as, used to
/// keep per-account caches (such as the account time zone) apart.
///
/// It names the credential source (profile, credentials file or ADC file)
/// and the impersonated user, so switching profile, credentials file or
/// `--impersonate` subject never reuses another identity's cached data.
/// Returns `Ok(None)` for a raw access token (`GWSR_TOKEN`,
/// `GWSR_TOKEN_FILE`), whose account is unknown: nothing may be cached for it.
///
/// # Errors
///
/// Credential-source resolution failures (conflicting or missing sources).
pub fn identity_cache_key() -> anyhow::Result<Option<String>> {
    let env = AuthEnv::from_process()?;
    Ok(identity_cache_key_for(&env)?)
}

fn identity_cache_key_for(env: &AuthEnv) -> Result<Option<String>, AuthError> {
    let source = env.source()?;
    let source = match &source {
        CredentialSource::TokenEnv | CredentialSource::TokenFile(_) => return Ok(None),
        CredentialSource::Profile { name, .. } => {
            format!("profile:{}:{name}", env.base.display())
        }
        CredentialSource::CredentialsFile(path)
        | CredentialSource::AdcEnv(path)
        | CredentialSource::AdcWellKnown(path) => {
            format!("{}:{}", source.kind(), path.display())
        }
    };
    Ok(Some(match &env.impersonate {
        Some(subject) => format!("{source}|impersonate:{subject}"),
        None => source,
    }))
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
        tracing::warn!(
            "this method needs one of [{}], but the active profile was granted none of \
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
/// then `quota_project_id` from Application Default Credentials when ADC is
/// the credential in use.
///
/// # Errors
///
/// A source that exists but cannot be used is an error, not skipped: skipping
/// it would silently bill a different project (or none). That covers an
/// unreadable or invalid OAuth client config and an unreadable or invalid
/// ADC file. (`GWSR_PROJECT_ID` itself is validated at startup by
/// [`crate::env`].)
/// `GWSR_NO_QUOTA_PROJECT` / `--no-quota-project` skip the lookup entirely.
pub fn get_quota_project() -> Result<Option<String>, GwsError> {
    let env = AuthEnv::from_process().map_err(to_gws_error)?;
    resolve_quota_project(
        crate::env::get()?.project_id.clone(),
        client_config::load_saved,
        adc_quota_source(&env)?.as_deref(),
    )
}

/// The ADC file whose `quota_project_id` may be used: only the one that is
/// this invocation's credential. A `quota_project_id` belongs to the ADC
/// identity; sending it with a profile, a credentials file or a token bills
/// (and permission-checks) a project unrelated to that credential.
fn adc_quota_source(env: &AuthEnv) -> Result<Option<std::path::PathBuf>, GwsError> {
    match env.source() {
        Ok(CredentialSource::AdcEnv(path) | CredentialSource::AdcWellKnown(path)) => Ok(Some(path)),
        Ok(_) | Err(AuthError::NoCredentials) => Ok(None),
        Err(e) => Err(to_gws_error(e.into())),
    }
}

fn resolve_quota_project(
    env_project: Option<String>,
    load_client_config: impl FnOnce() -> anyhow::Result<Option<client_config::ClientConfig>>,
    adc_path: Option<&std::path::Path>,
) -> Result<Option<String>, GwsError> {
    let skip = "set GWSR_PROJECT_ID, or pass --no-quota-project to send no quota project";
    if let Some(project) = env_project {
        return Ok(Some(project));
    }
    let client = load_client_config().map_err(|e| {
        to_gws_error(e.context(format!(
            "cannot read the OAuth client config for the quota project ({skip})"
        )))
    })?;
    if let Some(project) = client.and_then(|c| c.project_id).filter(|p| !p.is_empty()) {
        return Ok(Some(project));
    }
    let Some(path) = adc_path else {
        return Ok(None);
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(GwsError::Config(format!(
                "cannot read '{}' for the quota project ({skip}): {e}",
                path.display()
            )));
        }
    };
    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        GwsError::Config(format!(
            "'{}' is not valid JSON, so its quota_project_id cannot be read ({skip}): {e}",
            path.display()
        ))
    })?;
    Ok(json
        .get("quota_project_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::keystore::Purpose;
    use super::keystore::testing::memory_keystore;
    use super::profiles::{ActiveProfile, ProfileMetadata, ProfilePaths, ProfileSource};
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_with_project(project: Option<&str>) -> client_config::ClientConfig {
        client_config::ClientConfig {
            client_id: "id".to_string(),
            client_secret: secrecy::SecretString::from("secret".to_string()),
            project_id: project.map(str::to_string),
            source: client_config::ClientSource::BuiltIn,
        }
    }

    #[test]
    fn quota_project_priority_env_then_client_then_adc() {
        let dir = tempfile::tempdir().unwrap();
        let adc = dir.path().join("adc.json");
        std::fs::write(&adc, r#"{"quota_project_id":"from-adc"}"#).unwrap();
        let got = |env: Option<&str>, client: Option<&str>| {
            resolve_quota_project(
                env.map(str::to_string),
                || Ok(Some(client_with_project(client))),
                Some(&adc),
            )
            .unwrap()
        };
        assert_eq!(
            got(Some("from-env"), Some("c")).as_deref(),
            Some("from-env")
        );
        assert_eq!(
            got(None, Some("from-client")).as_deref(),
            Some("from-client")
        );
        assert_eq!(got(None, None).as_deref(), Some("from-adc"));
        let missing = dir.path().join("missing.json");
        assert_eq!(
            resolve_quota_project(None, || Ok(None), Some(&missing)).unwrap(),
            None
        );
        assert_eq!(
            resolve_quota_project(None, || Ok(None), None).unwrap(),
            None
        );
    }

    /// The ADC file's `quota_project_id` belongs to the ADC identity. It must
    /// not be sent for a gwsr profile, a credentials file or a token.
    #[test]
    fn adc_quota_project_only_applies_to_adc_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let adc = dir.path().join("adc.json");
        crate::fs_util::atomic_write(
            &adc,
            br#"{"type":"authorized_user","client_id":"c","client_secret":"s","refresh_token":"r","quota_project_id":"unrelated-adc-project"}"#,
        )
        .unwrap();
        let (ks, _) = memory_keystore(dir.path());
        let mut env = AuthEnv::new(
            dir.path().to_path_buf(),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(dir.path(), "default"),
        )
        .with_keystore(ks);
        env.adc_well_known = Some(adc.clone());
        let quota = |env: &AuthEnv| {
            resolve_quota_project(
                None,
                || Ok(Some(client_with_project(None))),
                adc_quota_source(env).unwrap().as_deref(),
            )
            .unwrap()
        };

        // ADC is the credential: its quota project applies.
        assert_eq!(quota(&env).as_deref(), Some("unrelated-adc-project"));

        // A logged-in profile (no project_id in client_secret.json): none.
        env.paths.ensure_dir().unwrap();
        let ct = env
            .keystore()
            .encrypt(
                Purpose::Credentials,
                br#"{"type":"authorized_user","client_id":"c","client_secret":"s","refresh_token":"r"}"#,
            )
            .unwrap();
        crate::fs_util::atomic_write(&env.paths.credentials, &ct).unwrap();
        assert_eq!(
            quota(&env),
            None,
            "profile credentials must not use ADC's quota project"
        );

        // GWSR_TOKEN: none either.
        env.token = Some(secrecy::SecretString::from("ya29.t".to_string()));
        assert_eq!(
            quota(&env),
            None,
            "a raw token must not use ADC's quota project"
        );

        // No credentials at all: nothing to bill.
        let empty = AuthEnv::new(
            dir.path().join("empty"),
            ActiveProfile {
                name: "default".into(),
                source: ProfileSource::Default,
            },
            ProfilePaths::new(&dir.path().join("empty"), "default"),
        );
        assert_eq!(adc_quota_source(&empty).unwrap(), None);
    }

    #[test]
    fn unusable_quota_project_sources_are_errors_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        // Unreadable OAuth client config.
        let err = resolve_quota_project(
            None,
            || Err(anyhow::anyhow!("client_secret.json: expected value")),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, GwsError::Config(_)), "{err:?}");
        assert!(err.to_string().contains("OAuth client config"), "{err}");
        let err = resolve_quota_project(
            None,
            || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, GwsError::CredentialStore(_)), "{err:?}");
        // Invalid or unreadable ADC.
        let adc = dir.path().join("adc.json");
        std::fs::write(&adc, "not json").unwrap();
        let err = resolve_quota_project(None, || Ok(None), Some(&adc)).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
        let err = resolve_quota_project(None, || Ok(None), Some(dir.path())).unwrap_err();
        assert!(matches!(err, GwsError::Config(_)), "{err:?}");
    }

    #[test]
    fn auth_errors_map_to_distinct_cli_errors_and_exit_codes() {
        use keystore::KeystoreError;
        let io = || std::io::Error::other("disk full");
        let cases: Vec<(AuthError, i32, &str)> = vec![
            (AuthError::NoCredentials, 2, "authError"),
            (
                AuthError::ProfileNotLoggedIn {
                    profile: "work".into(),
                },
                2,
                "authError",
            ),
            (
                AuthError::InvalidGrant {
                    description: "expired".into(),
                    login_command: "gwsr auth login".into(),
                },
                2,
                "authError",
            ),
            (
                AuthError::InvalidScope {
                    requested: vec!["https://www.googleapis.com/auth/drive".into()],
                    description: "nope".into(),
                },
                2,
                "authError",
            ),
            (
                AuthError::TokenEndpoint("unauthorized_client: nope".into()),
                2,
                "authError",
            ),
            (
                AuthError::TokenEndpointUnavailable {
                    status: 503,
                    detail: "Service Unavailable".into(),
                },
                6,
                "tokenEndpointUnavailable",
            ),
            (AuthError::Denied("access_denied".into()), 2, "authError"),
            (AuthError::Config("bad file".into()), 8, "configError"),
            (
                AuthError::Keystore(KeystoreError::KeyUnavailable("locked".into())),
                9,
                "credentialStoreError",
            ),
            (
                AuthError::Keystore(KeystoreError::Io {
                    context: "cannot read key".into(),
                    source: io(),
                }),
                9,
                "credentialStoreError",
            ),
            (
                AuthError::Storage("unreadable".into()),
                9,
                "credentialStoreError",
            ),
            (AuthError::Network("dns".into()), 10, "networkError"),
            (AuthError::Input("bad paste".into()), 3, "validationError"),
            (
                AuthError::Internal("task panicked".into()),
                5,
                "internalError",
            ),
        ];
        for (auth, exit, reason) in cases {
            let text = auth.to_string();
            // Wrapped in anyhow context, as the auth layer returns it.
            let wrapped = to_gws_error(anyhow::Error::new(auth).context("while loading"));
            assert_eq!(wrapped.exit_code(), exit, "{text}");
            assert_eq!(wrapped.to_json()["error"]["reason"], reason, "{text}");
            assert!(wrapped.to_string().contains("while loading"), "{wrapped}");
            assert!(wrapped.to_string().contains(&text), "{wrapped}");
        }
    }

    #[test]
    fn untyped_auth_layer_errors_are_config_and_io_errors_are_store_errors() {
        let config = to_gws_error(anyhow::anyhow!("environment variable X is not valid UTF-8"));
        assert!(matches!(config, GwsError::Config(_)), "{config:?}");
        let store = to_gws_error(
            anyhow::Error::new(std::io::Error::other("permission denied"))
                .context("cannot read '/x/metadata.json'"),
        );
        assert!(matches!(store, GwsError::CredentialStore(_)), "{store:?}");
        assert!(store.to_string().contains("permission denied"), "{store}");
        let direct: GwsError = keystore::KeystoreError::Decrypt { what: "x".into() }.into();
        assert!(matches!(direct, GwsError::CredentialStore(_)), "{direct:?}");
    }

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
            let t = get_token_fresh(&env, &scopes, Freshness::Cached)
                .await
                .unwrap();
            assert_eq!(t.expose_secret(), "ya29.ro");
        }
        assert!(env.paths.token_cache.exists());
        assert_eq!(
            granted_scopes_for(&env).unwrap(),
            Some(vec!["gmail.readonly".into(), "openid".into()])
        );
    }

    fn write_user_creds(env: &AuthEnv, refresh_token: &str) {
        let json = format!(
            r#"{{"type":"authorized_user","client_id":"cid","client_secret":"cs","refresh_token":"{refresh_token}"}}"#
        );
        let ct = env
            .keystore()
            .encrypt(Purpose::Credentials, json.as_bytes())
            .unwrap();
        crate::fs_util::atomic_write(&env.paths.credentials, &ct).unwrap();
    }

    fn token_reply(access_token: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": access_token, "expires_in": 3600, "token_type": "Bearer"
        }))
    }

    /// A refresh that loaded the previous grant's refresh token, and only
    /// took the cache lock after re-login cleared the cache, caches the old
    /// grant's access token. The next lookup must not be served that token.
    #[tokio::test]
    async fn token_cached_from_a_previous_grant_is_not_served_after_relogin() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token=rtold"))
            .respond_with(token_reply("ya29.old-grant"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token=rtnew"))
            .respond_with(token_reply("ya29.new-grant"))
            .expect(1)
            .mount(&server)
            .await;
        let env = profile_env(dir.path(), &server);
        write_user_creds(&env, "rtold");

        // Re-login holds the cache lock while a concurrent command, having
        // already loaded the old credentials, waits for it.
        let login_lock = crate::fs_util::FileLock::acquire(
            &env.paths.token_lock,
            std::time::Duration::from_secs(1),
        )
        .unwrap();
        let stale_env = env.clone();
        let stale =
            tokio::spawn(async move { get_token_fresh(&stale_env, &[], Freshness::Cached).await });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(
            !stale.is_finished(),
            "the refresh must be waiting for the lock"
        );
        write_user_creds(&env, "rtnew");
        match std::fs::remove_file(&env.paths.token_cache) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{e}"),
        }
        drop(login_lock);
        let old = stale.await.unwrap().unwrap();
        assert_eq!(old.expose_secret(), "ya29.old-grant");

        let next = get_token_fresh(&env, &[], Freshness::Cached).await.unwrap();
        assert_eq!(
            next.expose_secret(),
            "ya29.new-grant",
            "the previous grant's cached token was served after re-login"
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
        let err = get_token_fresh(&env, &["x".to_string()], Freshness::Cached)
            .await
            .unwrap_err();
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
        let err: anyhow::Error = get_token_fresh(&env, &[], Freshness::Cached)
            .await
            .unwrap_err()
            .into();
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
            get_token_fresh(&env, &[], Freshness::Cached)
                .await
                .unwrap()
                .expose_secret(),
            "tok"
        );
        assert_eq!(granted_scopes_for(&env).unwrap(), None);
    }

    #[test]
    fn identity_cache_key_separates_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |name: &str| {
            let paths = ProfilePaths::new(dir.path(), name);
            paths.ensure_dir().unwrap();
            std::fs::write(&paths.credentials, b"x").unwrap();
            AuthEnv::new(
                dir.path().to_path_buf(),
                ActiveProfile {
                    name: name.into(),
                    source: ProfileSource::Flag,
                },
                paths,
            )
        };
        let key = |env: &AuthEnv| identity_cache_key_for(env).unwrap();
        let work = key(&mk("work"));
        let personal = key(&mk("personal"));
        assert!(work.is_some());
        assert_ne!(work, personal, "profiles");

        let sa = dir.path().join("sa.json");
        std::fs::write(&sa, "{}").unwrap();
        let mut as_alice = mk("default");
        as_alice.profile.source = ProfileSource::Default;
        as_alice.credentials_file = Some(sa);
        as_alice.impersonate = Some("alice@example.com".into());
        let mut as_bob = as_alice.clone();
        as_bob.impersonate = Some("bob@example.com".into());
        assert_ne!(key(&as_alice), key(&as_bob), "impersonation subjects");
        assert_ne!(key(&as_alice), key(&mk("default")), "sources");

        let mut token = mk("default");
        // An explicit --profile conflicts with GWSR_TOKEN, so the token case
        // uses the default profile.
        token.profile.source = ProfileSource::Default;
        token.token = Some(SecretString::from("tok".to_string()));
        assert_eq!(key(&token), None, "a raw token has no known account");
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
            login_command(&mk("work", ProfileSource::ConfigFile)),
            "gwsr auth login --profile work"
        );
    }
}
