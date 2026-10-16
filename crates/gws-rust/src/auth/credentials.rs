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
//!    (`%APPDATA%\gcloud\application_default_credentials.json` on Windows)
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
use super::keystore::{BackendKind, Keystore, Purpose};
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
/// Application Default Credentials locations from the process:
/// `GOOGLE_APPLICATION_CREDENTIALS` and gcloud's well-known file.
#[cfg(not(test))]
fn process_adc_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    let from_env = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let well_known = gcloud_adc_file(cfg!(windows), dirs::home_dir(), dirs::config_dir());
    (from_env, well_known)
}

/// gcloud's Application Default Credentials file. gcloud keeps its
/// configuration in `~/.config/gcloud` on Linux and macOS, but in
/// `%APPDATA%\gcloud` (the roaming application-data directory, `roaming`)
/// on Windows, where nothing reads `~/.config`.
fn gcloud_adc_file(
    windows: bool,
    home: Option<PathBuf>,
    roaming: Option<PathBuf>,
) -> Option<PathBuf> {
    let config = if windows {
        roaming
    } else {
        home.map(|h| h.join(".config"))
    };
    config.map(|c| {
        c.join("gcloud")
            .join("application_default_credentials.json")
    })
}

/// Unit tests never read the developer's gcloud credentials.
#[cfg(test)]
fn process_adc_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    (None, None)
}

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
    /// `GWSR_KEYRING_BACKEND`.
    pub keyring_backend: BackendKind,
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

impl AuthEnv {
    /// Read the process environment and global flags.
    ///
    /// # Errors
    ///
    /// Config-dir or profile resolution failures.
    pub fn from_process() -> anyhow::Result<Self> {
        let vars = crate::env::get()?;
        let (base, profile, paths) = super::profiles::active_profile_paths()?;
        let mut env = Self::new(base, profile, paths);
        env.token = vars.token.clone();
        env.token_file = vars.token_file.clone();
        env.credentials_file = vars.credentials_file.clone();
        env.keyring_backend = vars.keyring_backend.unwrap_or_default();
        (env.adc_env, env.adc_well_known) = process_adc_paths();
        env.impersonate = super::profiles::impersonation_subject()?;
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
            keyring_backend: BackendKind::default(),
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
    /// [`Self::keyring_backend`]), created on first use.
    pub fn keystore(&self) -> Arc<Keystore> {
        Arc::clone(
            self.keystore
                .get_or_init(|| Arc::new(Keystore::for_kind(&self.base, self.keyring_backend))),
        )
    }

    /// Whether the profile was chosen explicitly (not the implicit default).
    pub fn profile_is_explicit(&self) -> bool {
        self.profile.source != ProfileSource::Default
    }

    /// The profile selection, as the user wrote it, when it was made for this
    /// command (`--profile`, or `GWSR_PROFILE`, which means the same): another
    /// identity (`GWSR_TOKEN`, `GWSR_TOKEN_FILE`, `GWSR_CREDENTIALS_FILE`)
    /// conflicts with it instead of silently replacing it. A profile pinned
    /// in `config.toml` is a standing default that those sources override.
    fn per_command_profile(&self) -> Option<String> {
        match self.profile.source {
            ProfileSource::Flag => Some(format!("--profile {}", self.profile.name)),
            ProfileSource::EnvVar => Some(format!("GWSR_PROFILE={}", self.profile.name)),
            ProfileSource::ConfigFile | ProfileSource::Default => None,
        }
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
            if let Some(selection) = self.per_command_profile() {
                let var = if self.token.is_some() {
                    "GWSR_TOKEN"
                } else {
                    "GWSR_TOKEN_FILE"
                };
                return Err(AuthError::Config(format!(
                    "{selection} conflicts with {var}; unset one of them"
                )));
            }
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
            if let Some(selection) = self.per_command_profile() {
                return Err(AuthError::Config(format!(
                    "{selection} conflicts with GWSR_CREDENTIALS_FILE; unset one of them"
                )));
            }
            if !path_exists(path, "GWSR_CREDENTIALS_FILE", AuthError::Config)? {
                return Err(AuthError::Config(format!(
                    "GWSR_CREDENTIALS_FILE points to '{}', which does not exist",
                    path.display()
                )));
            }
            return Ok(CredentialSource::CredentialsFile(path.clone()));
        }

        // An unreadable profile directory is an error, never "not logged
        // in": the default profile would otherwise fall through to
        // Application Default Credentials, another identity.
        let profile_what = format!("the credentials of profile '{}'", self.profile.name);
        if path_exists(&self.paths.credentials, &profile_what, AuthError::Storage)? {
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
            if !path_exists(path, "GOOGLE_APPLICATION_CREDENTIALS", AuthError::Config)? {
                return Err(AuthError::Config(format!(
                    "GOOGLE_APPLICATION_CREDENTIALS points to '{}', which does not exist",
                    path.display()
                )));
            }
            return Ok(CredentialSource::AdcEnv(path.clone()));
        }
        if let Some(path) = &self.adc_well_known
            && path_exists(
                path,
                "gcloud's Application Default Credentials",
                AuthError::Config,
            )?
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
            .keystore()
            .decrypt(Purpose::Credentials, &data, &what)?;
        let json = std::str::from_utf8(&plaintext)
            .map_err(|_| AuthError::Storage(format!("{what} decrypted to invalid UTF-8")))?;
        Credential::from_json(json, &what).map_err(|e| AuthError::Storage(format!("{e:#}")))
    }
}

/// Whether `path` exists. Unlike [`Path::exists`], a failure to check
/// (permission denied on a parent directory, ...) is an error built with
/// `kind`, not "absent": treating it as absent would silently pick the next
/// credential source, another identity.
fn path_exists(path: &Path, what: &str, kind: fn(String) -> AuthError) -> Result<bool, AuthError> {
    path.try_exists()
        .map_err(|e| kind(format!("cannot check {what} at '{}': {e}", path.display())))
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
    // The same rule as GWSR_TOKEN: the token goes into an HTTP header. The
    // value itself is never echoed.
    crate::env::check_printable(token).map_err(|problem| {
        AuthError::Config(format!(
            "GWSR_TOKEN_FILE '{}' {problem}; it must hold exactly one access token",
            path.display()
        ))
    })?;
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
    use super::super::keystore::KeystoreError;
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

    /// `gcloud auth application-default login` writes to `%APPDATA%\gcloud`
    /// on Windows; looking in `~/.config/gcloud` there never finds it.
    #[test]
    fn gcloud_adc_file_follows_gclouds_config_dir() {
        let home = PathBuf::from("home");
        let roaming = PathBuf::from("roaming");
        let file = |windows| gcloud_adc_file(windows, Some(home.clone()), Some(roaming.clone()));
        assert_eq!(
            file(true),
            Some(
                roaming
                    .join("gcloud")
                    .join("application_default_credentials.json")
            )
        );
        assert_eq!(
            file(false),
            Some(
                home.join(".config")
                    .join("gcloud")
                    .join("application_default_credentials.json")
            )
        );
        assert_eq!(gcloud_adc_file(true, Some(home), None), None);
        assert_eq!(gcloud_adc_file(false, None, Some(roaming)), None);
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

    /// GWSR_TOKEN_FILE follows the same rule as GWSR_TOKEN: a value with
    /// inner whitespace, control or non-ASCII characters (two tokens, a JSON
    /// token response, a pasted `Bearer ` prefix) is refused with a config
    /// error that names the file and never echoes the value.
    #[test]
    fn token_file_with_more_than_one_token_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.token_file = Some(path.clone());
        for bad in [
            "ya29.first\nya29.second\n",
            "Bearer ya29.secret",
            "{\"access_token\": \"ya29.secret\"}",
            "ya29.tab\tsecret",
            "ya29.ünicode",
        ] {
            write_private(&path, bad);
            let err = env.load().unwrap_err();
            assert!(matches!(err, AuthError::Config(_)), "{bad:?}: {err:?}");
            let text = err.to_string();
            assert!(text.contains("GWSR_TOKEN_FILE"), "{text}");
            assert!(!text.contains("ya29"), "the value is echoed: {text}");
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

    /// An explicit `--profile` must never be silently replaced by another
    /// identity: a pre-obtained token conflicts with it exactly like
    /// GWSR_CREDENTIALS_FILE does.
    #[test]
    fn token_env_and_token_file_conflict_with_profile_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        write_private(&path, "ya29.abc");
        let mut env = env_in(dir.path(), ProfileSource::Flag);
        env.token = Some(SecretString::from("tok".to_string()));
        let err = env.source().unwrap_err();
        assert!(matches!(err, AuthError::Config(_)), "{err:?}");
        assert!(err.to_string().contains("GWSR_TOKEN"), "{err}");

        env.token = None;
        env.token_file = Some(path);
        let err = env.source().unwrap_err();
        assert!(matches!(err, AuthError::Config(_)), "{err:?}");
        assert!(err.to_string().contains("GWSR_TOKEN_FILE"), "{err}");

        // Without --profile the token still wins.
        let mut env = env_in(dir.path(), ProfileSource::Default);
        env.token = Some(SecretString::from("tok".to_string()));
        assert_eq!(env.source().unwrap(), CredentialSource::TokenEnv);
    }

    /// `GWSR_PROFILE` selects a profile as explicitly as `--profile` ("same
    /// as --profile"), so another identity from GWSR_TOKEN, GWSR_TOKEN_FILE
    /// or GWSR_CREDENTIALS_FILE conflicts with it instead of silently
    /// replacing it. A profile pinned in config.toml (`gwsr auth use`) is a
    /// standing default, not a per-command choice: those sources still win
    /// over it.
    #[test]
    fn other_identities_conflict_with_gwsr_profile_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        write_private(&token_path, "ya29.abc");
        let sa_path = dir.path().join("sa.json");
        write_private(&sa_path, SA_JSON);

        let set = |env: &mut AuthEnv, var: &str| match var {
            "GWSR_TOKEN" => env.token = Some(SecretString::from("tok".to_string())),
            "GWSR_TOKEN_FILE" => env.token_file = Some(token_path.clone()),
            _ => env.credentials_file = Some(sa_path.clone()),
        };
        for var in ["GWSR_TOKEN", "GWSR_TOKEN_FILE", "GWSR_CREDENTIALS_FILE"] {
            let mut env = env_in(dir.path(), ProfileSource::EnvVar);
            set(&mut env, var);
            let err = env.source().unwrap_err();
            assert!(matches!(err, AuthError::Config(_)), "{var}: {err:?}");
            assert!(
                err.to_string().contains(&format!(
                    "GWSR_PROFILE={DEFAULT_PROFILE} conflicts with {var}"
                )),
                "{err}"
            );

            let mut env = env_in(dir.path(), ProfileSource::ConfigFile);
            set(&mut env, var);
            assert!(env.source().is_ok(), "{var} with a config.toml profile");
        }
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

    /// A profile directory that cannot be searched (left owned by root by a
    /// `sudo gwsr auth login`, say) hides whether credentials exist. That is
    /// a storage error, not "not logged in": the implicit default profile
    /// must not fall through to Application Default Credentials, another
    /// identity, and an explicit profile must not claim it has none.
    #[cfg(unix)]
    #[test]
    fn unreadable_profile_dir_is_an_error_not_an_adc_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (ks, _) = memory_keystore(dir.path());
        let env = env_in(dir.path(), ProfileSource::Default);
        write_profile_creds(&env, &ks);
        let adc = dir.path().join("adc.json");
        write_private(&adc, SA_JSON);
        let profile_dir = env.paths.dir.clone();
        std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root ignores directory permissions; nothing to test then.
        let enforced = std::fs::metadata(&env.paths.credentials).is_err();

        let mut implicit = env.clone();
        implicit.adc_well_known = Some(adc.clone());
        let implicit_result = implicit.source();
        let mut via_env = env.clone();
        via_env.adc_env = Some(adc);
        let via_env_result = via_env.source();
        let explicit_result = env_in(dir.path(), ProfileSource::EnvVar).source();
        std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        if !enforced {
            return;
        }

        for result in [implicit_result, via_env_result, explicit_result] {
            let err = result.unwrap_err();
            assert!(matches!(err, AuthError::Storage(_)), "{err:?}");
            assert!(err.to_string().contains("profile 'default'"), "{err}");
        }
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
