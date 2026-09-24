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

//! Credential types and credential-source resolution.
//!
//! Sources, in priority order:
//!
//! 1. `GWSR_TOKEN` (raw access token) or `GWSR_TOKEN_FILE` (file holding one)
//! 2. `GWSR_CREDENTIALS_FILE` (authorized-user or service-account JSON)
//! 3. the active profile's encrypted credentials (`gwsr auth login`)
//! 4. `GOOGLE_APPLICATION_CREDENTIALS`
//! 5. `~/.config/gcloud/application_default_credentials.json`
//!
//! Once a source exists it is used or the command fails: a broken encrypted
//! credentials file never silently falls through to another identity, and a
//! profile chosen explicitly (`--profile`, `GWSR_PROFILE`, `gwsr auth use`)
//! never falls back to Application Default Credentials.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use zeroize::Zeroizing;

use super::AuthError;
use super::keystore::{Keystore, KeystoreError, Purpose};
use super::profiles::{ActiveProfile, ProfilePaths, ProfileSource};

/// An OAuth user credential (refresh token).
#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizedUser {
    pub client_id: String,
    pub client_secret: SecretString,
    pub refresh_token: SecretString,
}

/// A service-account key.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccountKey {
    pub client_email: String,
    pub private_key: SecretString,
    #[serde(default)]
    pub private_key_id: Option<String>,
    /// Numeric OAuth client ID of the service account (used for DWD setup).
    #[serde(default)]
    pub client_id: Option<String>,
}

/// A long-lived credential. `Debug` never prints secrets.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    AuthorizedUser(AuthorizedUser),
    ServiceAccount(ServiceAccountKey),
}

impl Credential {
    /// Parse credential JSON. Only `authorized_user` and `service_account`
    /// are supported; anything else is an explicit error.
    ///
    /// # Errors
    ///
    /// Invalid JSON, a missing/unsupported `type`, or missing fields.
    pub fn from_json(json: &str, origin: &str) -> anyhow::Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(json).with_context(|| format!("{origin} is not valid JSON"))?;
        match value.get("type").and_then(|t| t.as_str()) {
            Some("authorized_user" | "service_account") => {}
            Some(other) => anyhow::bail!(
                "{origin} has credential type '{other}', which gwsr does not support \
                 (supported: authorized_user, service_account)"
            ),
            None => anyhow::bail!(
                "{origin} has no \"type\" field (expected \"authorized_user\" or \
                 \"service_account\")"
            ),
        }
        serde_json::from_value(value)
            .with_context(|| format!("{origin} is missing required credential fields"))
    }

    /// The identity used in cache keys and status output (never secret).
    pub fn identity(&self) -> String {
        match self {
            Credential::AuthorizedUser(u) => format!("user:{}", u.client_id),
            Credential::ServiceAccount(sa) => format!("sa:{}", sa.client_email),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Credential::AuthorizedUser(_) => "authorized_user",
            Credential::ServiceAccount(_) => "service_account",
        }
    }
}

/// Serialize an authorized-user credential into the standard gcloud format.
pub fn authorized_user_json(
    client_id: &str,
    client_secret: &SecretString,
    refresh_token: &SecretString,
) -> Zeroizing<String> {
    Zeroizing::new(
        serde_json::json!({
            "type": "authorized_user",
            "client_id": client_id,
            "client_secret": client_secret.expose_secret(),
            "refresh_token": refresh_token.expose_secret(),
        })
        .to_string(),
    )
}

/// Where the credential for this invocation comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// `GWSR_TOKEN`
    TokenEnv,
    /// `GWSR_TOKEN_FILE`
    TokenFile(PathBuf),
    /// `GWSR_CREDENTIALS_FILE`
    CredentialsFile(PathBuf),
    /// The profile's encrypted credentials.
    Profile { name: String, path: PathBuf },
    /// `GOOGLE_APPLICATION_CREDENTIALS`
    AdcEnv(PathBuf),
    /// gcloud's well-known ADC file.
    AdcWellKnown(PathBuf),
}

impl CredentialSource {
    /// Stable identifier for JSON output.
    pub fn kind(&self) -> &'static str {
        match self {
            CredentialSource::TokenEnv => "token_env_var",
            CredentialSource::TokenFile(_) => "token_file",
            CredentialSource::CredentialsFile(_) => "credentials_file_env_var",
            CredentialSource::Profile { .. } => "profile",
            CredentialSource::AdcEnv(_) => "google_application_credentials",
            CredentialSource::AdcWellKnown(_) => "gcloud_adc",
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            CredentialSource::TokenEnv => None,
            CredentialSource::TokenFile(p)
            | CredentialSource::CredentialsFile(p)
            | CredentialSource::AdcEnv(p)
            | CredentialSource::AdcWellKnown(p) => Some(p),
            CredentialSource::Profile { path, .. } => Some(path),
        }
    }
}

/// A resolved credential ready for token acquisition.
#[derive(Debug, Clone)]
pub enum Resolved {
    /// A pre-obtained access token.
    Token(SecretString),
    Credential(Credential),
}

/// Snapshot of everything that influences credential resolution. Built from
/// the process environment in production and by hand in tests.
#[derive(Clone)]
pub struct AuthEnv {
    pub base: PathBuf,
    pub profile: ActiveProfile,
    pub paths: ProfilePaths,
    pub token: Option<SecretString>,
    pub token_file: Option<PathBuf>,
    pub credentials_file: Option<PathBuf>,
    pub adc_env: Option<PathBuf>,
    pub adc_well_known: Option<PathBuf>,
    pub impersonate: Option<String>,
    pub endpoints: super::http::Endpoints,
    keystore: Arc<OnceLock<Arc<Keystore>>>,
}

impl std::fmt::Debug for AuthEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthEnv")
            .field("base", &self.base)
            .field("profile", &self.profile)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("token_file", &self.token_file)
            .field("credentials_file", &self.credentials_file)
            .field("adc_env", &self.adc_env)
            .field("adc_well_known", &self.adc_well_known)
            .field("impersonate", &self.impersonate)
            .finish_non_exhaustive()
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl AuthEnv {
    /// Read the process environment and global flags.
    ///
    /// # Errors
    ///
    /// Config-dir or profile resolution failures.
    pub fn from_process() -> anyhow::Result<Self> {
        let (base, profile, paths) = super::profiles::active_profile_paths()?;
        let mut env = Self::new(base, profile, paths);
        env.token = std::env::var("GWSR_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .map(SecretString::from);
        env.token_file = env_path("GWSR_TOKEN_FILE");
        env.credentials_file = env_path("GWSR_CREDENTIALS_FILE");
        env.adc_env = env_path("GOOGLE_APPLICATION_CREDENTIALS");
        // gcloud always uses ~/.config/gcloud, even on macOS and Windows.
        env.adc_well_known = dirs::home_dir().map(|h| {
            h.join(".config")
                .join("gcloud")
                .join("application_default_credentials.json")
        });
        env.impersonate = super::profiles::impersonation_subject();
        Ok(env)
    }

    /// An environment with no credential sources set.
    pub fn new(base: PathBuf, profile: ActiveProfile, paths: ProfilePaths) -> Self {
        Self {
            base,
            profile,
            paths,
            token: None,
            token_file: None,
            credentials_file: None,
            adc_env: None,
            adc_well_known: None,
            impersonate: None,
            endpoints: super::http::Endpoints::default(),
            keystore: Arc::new(OnceLock::new()),
        }
    }

    /// Use a specific keystore (tests).
    #[cfg(test)]
    pub fn with_keystore(mut self, keystore: Keystore) -> Self {
        self.keystore = Arc::new(OnceLock::from(Arc::new(keystore)));
        self
    }

    /// The keystore for this config directory (backend from
    /// `GWSR_KEYRING_BACKEND`), created on first use.
    ///
    /// # Errors
    ///
    /// Invalid `GWSR_KEYRING_BACKEND`.
    pub fn keystore(&self) -> Result<Arc<Keystore>, KeystoreError> {
        if let Some(ks) = self.keystore.get() {
            return Ok(Arc::clone(ks));
        }
        let ks = Arc::new(Keystore::from_env(&self.base)?);
        Ok(Arc::clone(self.keystore.get_or_init(|| ks)))
    }

    /// Whether the profile was chosen explicitly (not the implicit default).
    pub fn profile_is_explicit(&self) -> bool {
        self.profile.source != ProfileSource::Default
    }

    /// Determine which source applies, without reading secrets.
    ///
    /// # Errors
    ///
    /// Conflicting settings, or an explicitly configured source that does not
    /// exist.
    pub fn source(&self) -> Result<CredentialSource, AuthError> {
        if self.token.is_some() && self.token_file.is_some() {
            return Err(AuthError::Config(
                "both GWSR_TOKEN and GWSR_TOKEN_FILE are set; set only one".into(),
            ));
        }
        let direct = if self.token.is_some() {
            Some(CredentialSource::TokenEnv)
        } else {
            self.token_file.clone().map(CredentialSource::TokenFile)
        };
        if let Some(src) = direct {
            if self.impersonate.is_some() {
                return Err(AuthError::Config(
                    "--impersonate/GWSR_IMPERSONATE cannot be used with a pre-obtained access \
                     token (GWSR_TOKEN/GWSR_TOKEN_FILE)"
                        .into(),
                ));
            }
            return Ok(src);
        }

        if let Some(path) = &self.credentials_file {
            if self.profile.source == ProfileSource::Flag {
                return Err(AuthError::Config(format!(
                    "--profile {} conflicts with GWSR_CREDENTIALS_FILE; unset one of them",
                    self.profile.name
                )));
            }
            if !path.exists() {
                return Err(AuthError::Config(format!(
                    "GWSR_CREDENTIALS_FILE points to '{}', which does not exist",
                    path.display()
                )));
            }
            return Ok(CredentialSource::CredentialsFile(path.clone()));
        }

        if self.paths.credentials.exists() {
            return Ok(CredentialSource::Profile {
                name: self.profile.name.clone(),
                path: self.paths.credentials.clone(),
            });
        }
        if self.profile_is_explicit() {
            return Err(AuthError::ProfileNotLoggedIn {
                profile: self.profile.name.clone(),
            });
        }

        if let Some(path) = &self.adc_env {
            if !path.exists() {
                return Err(AuthError::Config(format!(
                    "GOOGLE_APPLICATION_CREDENTIALS points to '{}', which does not exist",
                    path.display()
                )));
            }
            return Ok(CredentialSource::AdcEnv(path.clone()));
        }
        if let Some(path) = &self.adc_well_known
            && path.exists()
        {
            return Ok(CredentialSource::AdcWellKnown(path.clone()));
        }
        Err(AuthError::NoCredentials)
    }

    /// Resolve and load the credential.
    ///
    /// Blocking: reads files and may access the OS keyring. Call from
    /// `spawn_blocking` in async code.
    ///
    /// # Errors
    ///
    /// Any failure to read, decrypt or parse the chosen source.
    pub fn load(&self) -> Result<(CredentialSource, Resolved), AuthError> {
        let source = self.source()?;
        let resolved = match &source {
            CredentialSource::TokenEnv => match &self.token {
                Some(t) => Resolved::Token(t.clone()),
                None => return Err(AuthError::NoCredentials),
            },
            CredentialSource::TokenFile(path) => Resolved::Token(read_token_file(path)?),
            CredentialSource::CredentialsFile(path)
            | CredentialSource::AdcEnv(path)
            | CredentialSource::AdcWellKnown(path) => {
                Resolved::Credential(read_plaintext_credential(path)?)
            }
            CredentialSource::Profile { path, name } => {
                Resolved::Credential(self.read_profile_credential(path, name)?)
            }
        };
        if self.impersonate.is_some()
            && matches!(
                resolved,
                Resolved::Credential(Credential::AuthorizedUser(_))
            )
        {
            return Err(AuthError::Config(format!(
                "--impersonate/GWSR_IMPERSONATE requires service-account credentials, but {} \
                 holds a user (authorized_user) credential",
                source
                    .path()
                    .map(|p| format!("'{}'", p.display()))
                    .unwrap_or_else(|| source.kind().to_string())
            )));
        }
        Ok((source, resolved))
    }

    fn read_profile_credential(&self, path: &Path, name: &str) -> Result<Credential, AuthError> {
        let data = std::fs::read(path)
            .map_err(|e| AuthError::Storage(format!("cannot read '{}': {e}", path.display())))?;
        let what = format!("the credentials of profile '{name}' ('{}')", path.display());
        let plaintext = self
            .keystore()?
            .decrypt(Purpose::Credentials, &data, &what)?;
        let json = std::str::from_utf8(&plaintext)
            .map_err(|_| AuthError::Storage(format!("{what} decrypted to invalid UTF-8")))?;
        Credential::from_json(json, &what).map_err(|e| AuthError::Storage(format!("{e:#}")))
    }
}

fn read_token_file(path: &Path) -> Result<SecretString, AuthError> {
    crate::fs_util::check_private_file(path).map_err(|e| AuthError::Config(format!("{e:#}")))?;
    let contents = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| {
        AuthError::Config(format!(
            "cannot read GWSR_TOKEN_FILE '{}': {e}",
            path.display()
        ))
    })?);
    let token = contents.trim();
    if token.is_empty() {
        return Err(AuthError::Config(format!(
            "GWSR_TOKEN_FILE '{}' is empty",
            path.display()
        )));
    }
    Ok(SecretString::from(token.to_string()))
}

fn read_plaintext_credential(path: &Path) -> Result<Credential, AuthError> {
    crate::fs_util::check_private_file(path).map_err(|e| AuthError::Config(format!("{e:#}")))?;
    let contents = Zeroizing::new(
        std::fs::read_to_string(path)
            .map_err(|e| AuthError::Config(format!("cannot read '{}': {e}", path.display())))?,
    );
    Credential::from_json(&contents, &format!("'{}'", path.display()))
        .map_err(|e| AuthError::Config(format!("{e:#}")))
}

#[cfg(test)]
mod tests {
    use super::super::keystore::BackendKind;
    use super::super::keystore::testing::{FailingBackend, memory_keystore};
    use super::super::profiles::DEFAULT_PROFILE;
    use super::*;

    const USER_JSON: &str = r#"{"type":"authorized_user","client_id":"cid","client_secret":"csecret-XYZ","refresh_token":"1//refresh-ABC"}"#;
    const SA_JSON: &str = r#"{"type":"service_account","client_email":"sa@p.iam.gserviceaccount.com","private_key":"-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n","project_id":"p"}"#;

    fn env_in(dir: &Path, source: ProfileSource) -> AuthEnv {
        let profile = ActiveProfile {
            name: DEFAULT_PROFILE.into(),
            source,
        };
        let paths = ProfilePaths::new(dir, DEFAULT_PROFILE);
        AuthEnv::new(dir.to_path_buf(), profile, paths)
    }

    fn write_private(path: &Path, contents: &str) {
        crate::fs_util::atomic_write(path, contents.as_bytes()).unwrap();
    }

    fn write_profile_creds(env: &AuthEnv, ks: &Keystore) {
        env.paths.ensure_dir().unwrap();
        let ct = ks
            .encrypt(Purpose::Credentials, USER_JSON.as_bytes())
            .unwrap();
        crate::fs_util::atomic_write(&env.paths.credentials, &ct).unwrap();
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let user = Credential::from_json(USER_JSON, "t").unwrap();
        let sa = Credential::from_json(SA_JSON, "t").unwrap();
        let dbg = format!("{user:?} {sa:?}");
        assert!(!dbg.contains("csecret-XYZ"), "{dbg}");
        assert!(!dbg.contains("refresh-ABC"), "{dbg}");
        assert!(!dbg.contains("AAAA"), "{dbg}");
        assert!(dbg.contains("cid"));
    }

    #[test]
    fn unsupported_or_missing_type_is_an_error() {
        let err = Credential::from_json(r#"{"type":"external_account"}"#, "f")
            .unwrap_err()
            .to_string();
        assert!(err.contains("external_account"), "{err}");
        assert!(Credential::from_json(r#"{"client_id":"x"}"#, "f").is_err());
        assert!(Credential::from_json(r#"{"type":"authorized_user"}"#, "f").is_err());
    }

    #[test]
    fn no_sources_is_no_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_in(dir.path(), ProfileSource::Default);
        assert!(matches!(env.source(), Err(AuthError::NoCredentials)));
    }

    #[test]
    fn token_env_wins_and_conflicts_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.token = Some(SecretString::from("tok".to_string()));
        assert_eq!(env.source().unwrap(), CredentialSource::TokenEnv);

        env.token_file = Some(dir.path().join("t"));
        assert!(matches!(env.source(), Err(AuthError::Config(_))));

        env.token_file = None;
        env.impersonate = Some("u@example.com".into());
        assert!(matches!(env.source(), Err(AuthError::Config(_))));
    }

    #[test]
    fn token_file_is_read_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        write_private(&path, "  ya29.abc\n");
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.token_file = Some(path.clone());
        match env.load().unwrap().1 {
            Resolved::Token(t) => assert_eq!(t.expose_secret(), "ya29.abc"),
            other => panic!("{other:?}"),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(env.load().is_err());
        }
    }

    #[test]
    fn credentials_file_missing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.credentials_file = Some(dir.path().join("nope.json"));
        assert!(matches!(env.source(), Err(AuthError::Config(_))));
    }

    #[cfg(unix)]
    #[test]
    fn world_readable_credentials_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sa.json");
        std::fs::write(&path, SA_JSON).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.credentials_file = Some(path);
        let err = env.load().unwrap_err().to_string();
        assert!(err.contains("chmod 600"), "{err}");
    }

    #[test]
    fn credentials_file_conflicts_with_profile_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sa.json");
        write_private(&path, SA_JSON);
        let mut env = env_in(dir.path(), ProfileSource::Flag);
        env.credentials_file = Some(path);
        assert!(matches!(env.source(), Err(AuthError::Config(_))));
    }

    #[test]
    fn profile_credentials_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (ks, _) = memory_keystore(dir.path());
        let env = env_in(dir.path(), ProfileSource::Default);
        write_profile_creds(&env, &ks);
        let env = env.with_keystore(ks);
        let (src, resolved) = env.load().unwrap();
        assert!(matches!(src, CredentialSource::Profile { .. }));
        match resolved {
            Resolved::Credential(Credential::AuthorizedUser(u)) => {
                assert_eq!(u.refresh_token.expose_secret(), "1//refresh-ABC");
            }
            other => panic!("{other:?}"),
        }
    }

    /// SEC-06 / P0-2: a keyring failure must neither delete the credentials
    /// nor fall through to another identity (ADC).
    #[test]
    fn keyring_failure_keeps_files_and_does_not_fall_back_to_adc() {
        let dir = tempfile::tempdir().unwrap();
        let (good, _) = memory_keystore(dir.path());
        let env = env_in(dir.path(), ProfileSource::Default);
        write_profile_creds(&env, &good);
        let cache = env.paths.token_cache.clone();
        std::fs::write(&cache, b"GWSR\x01cache-bytes-cache-bytes-cache-bytes").unwrap();
        let adc = dir.path().join("adc.json");
        write_private(&adc, SA_JSON);

        let mut env = env.with_keystore(Keystore::with_backend(
            dir.path(),
            BackendKind::Keyring,
            Box::new(FailingBackend),
        ));
        env.adc_env = Some(adc);
        let err = env.load().unwrap_err();
        assert!(
            matches!(err, AuthError::Keystore(KeystoreError::KeyUnavailable(_))),
            "{err}"
        );
        assert!(env.paths.credentials.exists(), "credentials must survive");
        assert!(cache.exists(), "token cache must survive");
    }

    /// SEC-06: corrupt ciphertext is an error, files survive, no ADC fallback.
    #[test]
    fn corrupt_credentials_fail_without_deleting_or_falling_back() {
        let dir = tempfile::tempdir().unwrap();
        let (ks, _) = memory_keystore(dir.path());
        ks.key_for_encryption().unwrap();
        let env = env_in(dir.path(), ProfileSource::Default);
        env.paths.ensure_dir().unwrap();
        std::fs::write(&env.paths.credentials, b"not-valid-encrypted-data-at-all").unwrap();
        let adc = dir.path().join("adc.json");
        write_private(&adc, SA_JSON);
        let mut env = env.with_keystore(ks);
        env.adc_env = Some(adc);

        let err = env.load().unwrap_err();
        assert!(
            matches!(
                err,
                AuthError::Keystore(KeystoreError::UnsupportedFormat { .. })
            ),
            "{err}"
        );
        assert_eq!(
            std::fs::read(&env.paths.credentials).unwrap(),
            b"not-valid-encrypted-data-at-all"
        );

        // Wrong key: authentication failure, still untouched.
        let other_dir = tempfile::tempdir().unwrap();
        let (other, _) = memory_keystore(other_dir.path());
        let ct = other
            .encrypt(Purpose::Credentials, USER_JSON.as_bytes())
            .unwrap();
        std::fs::write(&env.paths.credentials, &ct).unwrap();
        let err = env.load().unwrap_err();
        assert!(
            matches!(err, AuthError::Keystore(KeystoreError::Decrypt { .. })),
            "{err}"
        );
        assert_eq!(std::fs::read(&env.paths.credentials).unwrap(), ct);
    }

    #[test]
    fn explicit_profile_without_login_does_not_use_adc() {
        let dir = tempfile::tempdir().unwrap();
        let adc = dir.path().join("adc.json");
        write_private(&adc, SA_JSON);
        let mut env = env_in(dir.path(), ProfileSource::EnvVar);
        env.adc_env = Some(adc);
        assert!(matches!(
            env.source(),
            Err(AuthError::ProfileNotLoggedIn { .. })
        ));
    }

    #[test]
    fn implicit_default_profile_falls_back_to_adc() {
        let dir = tempfile::tempdir().unwrap();
        let adc = dir.path().join("adc.json");
        write_private(&adc, SA_JSON);
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.adc_well_known = Some(adc.clone());
        assert_eq!(
            env.source().unwrap(),
            CredentialSource::AdcWellKnown(adc.clone())
        );
        env.adc_env = Some(dir.path().join("missing.json"));
        assert!(matches!(env.source(), Err(AuthError::Config(_))));
    }

    #[test]
    fn impersonation_requires_service_account() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.json");
        write_private(&user, USER_JSON);
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.credentials_file = Some(user);
        env.impersonate = Some("alice@example.com".into());
        let err = env.load().unwrap_err().to_string();
        assert!(err.contains("service-account"), "{err}");

        let sa = dir.path().join("sa.json");
        write_private(&sa, SA_JSON);
        env.credentials_file = Some(sa);
        assert!(matches!(
            env.load().unwrap().1,
            Resolved::Credential(Credential::ServiceAccount(_))
        ));
    }
}
