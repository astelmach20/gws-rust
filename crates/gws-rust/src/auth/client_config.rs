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

//! The OAuth client ("Desktop app" client ID and secret) used for login.
//!
//! Resolution order:
//!
//! 1. `GWSR_CLIENT_ID` + `GWSR_CLIENT_SECRET` (both or neither)
//! 2. `<config>/client_secret.json` in the Google Cloud Console "installed"
//!    download format (must be private: 0600)
//! 3. a client compiled into this build via the build-time environment
//!    variables `GWSR_DEFAULT_CLIENT_ID` / `GWSR_DEFAULT_CLIENT_SECRET`
//!    (no client is hard-coded in the source)

use std::path::{Path, PathBuf};

use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

const BUILTIN_CLIENT_ID: Option<&str> = option_env!("GWSR_DEFAULT_CLIENT_ID");
const BUILTIN_CLIENT_SECRET: Option<&str> = option_env!("GWSR_DEFAULT_CLIENT_SECRET");

/// Where the OAuth client came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientSource {
    EnvVars,
    SavedFile(PathBuf),
    BuiltIn,
}

impl ClientSource {
    pub fn kind(&self) -> &'static str {
        match self {
            ClientSource::EnvVars => "environment_variables",
            ClientSource::SavedFile(_) => "client_secret.json",
            ClientSource::BuiltIn => "built_in",
        }
    }
}

/// A resolved OAuth client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    pub project_id: Option<String>,
    pub source: ClientSource,
}

#[derive(Serialize, Deserialize)]
struct InstalledConfig {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default = "default_auth_uri")]
    auth_uri: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
    #[serde(default)]
    redirect_uris: Vec<String>,
}

fn default_auth_uri() -> String {
    "https://accounts.google.com/o/oauth2/auth".to_string()
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

impl Drop for InstalledConfig {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.client_secret);
    }
}

#[derive(Serialize, Deserialize)]
struct ClientSecretFile {
    installed: InstalledConfig,
}

/// Path of the saved client configuration.
///
/// # Errors
///
/// See [`super::profiles::try_config_dir`].
pub fn client_config_path() -> anyhow::Result<PathBuf> {
    Ok(super::profiles::try_config_dir()?.join("client_secret.json"))
}

/// Save a client in the Google Cloud Console format (0600 file, 0700 dir).
///
/// # Errors
///
/// Directory creation or write failures.
pub fn save_client_config(
    path: &Path,
    client_id: &str,
    client_secret: &SecretString,
    project_id: Option<&str>,
) -> anyhow::Result<()> {
    check_field(path, "client_id", client_id)?;
    check_field(path, "client_secret", client_secret.expose_secret())?;
    let file = ClientSecretFile {
        installed: InstalledConfig {
            client_id: client_id.to_string(),
            client_secret: client_secret.expose_secret().to_string(),
            project_id: project_id.map(str::to_string),
            auth_uri: default_auth_uri(),
            token_uri: default_token_uri(),
            redirect_uris: vec!["http://127.0.0.1".to_string()],
        },
    };
    if let Some(parent) = path.parent() {
        crate::fs_util::ensure_private_dir(parent)
            .with_context(|| format!("cannot create '{}'", parent.display()))?;
    }
    let json = zeroize::Zeroizing::new(
        serde_json::to_vec_pretty(&file).context("cannot serialize the OAuth client config")?,
    );
    crate::fs_util::atomic_write(path, &json)
        .with_context(|| format!("cannot write '{}'", path.display()))
}

/// Reject an empty client ID/secret or one with whitespace, control or
/// non-ASCII characters (the same rule as `GWSR_CLIENT_ID`). A stray pasted
/// space otherwise reaches Google and fails as `invalid_client` ("The OAuth
/// client was not found"), which does not point at the file. The value itself
/// is never echoed.
fn check_field(path: &Path, field: &str, value: &str) -> anyhow::Result<()> {
    let problem = if value.is_empty() {
        "is empty".to_string()
    } else {
        match crate::env::check_printable(value) {
            Ok(()) => return Ok(()),
            Err(p) => p,
        }
    };
    anyhow::bail!(
        "\"{field}\" in '{}' {problem}; edit the file (for example remove a stray space pasted \
         with the value), or delete it and run `gwsr auth setup` again",
        path.display()
    )
}

/// Load a saved client configuration; `Ok(None)` if the file does not exist.
///
/// # Errors
///
/// The file is not private, unreadable or malformed.
pub fn load_from(path: &Path) -> anyhow::Result<Option<ClientConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    crate::fs_util::check_private_file(path)?;
    let data = zeroize::Zeroizing::new(
        std::fs::read_to_string(path)
            .with_context(|| format!("cannot read '{}'", path.display()))?,
    );
    let file: ClientSecretFile = serde_json::from_str(&data).with_context(|| {
        format!(
            "'{}' is not a Desktop-app OAuth client file (expected the \"installed\" format \
             downloaded from Google Cloud Console)",
            path.display()
        )
    })?;
    check_field(path, "client_id", &file.installed.client_id)?;
    check_field(path, "client_secret", &file.installed.client_secret)?;
    Ok(Some(ClientConfig {
        client_id: file.installed.client_id.clone(),
        client_secret: SecretString::from(file.installed.client_secret.clone()),
        project_id: file.installed.project_id.clone().filter(|p| !p.is_empty()),
        source: ClientSource::SavedFile(path.to_path_buf()),
    }))
}

/// Load the saved client configuration from the config directory.
///
/// # Errors
///
/// See [`load_from`].
pub fn load_saved() -> anyhow::Result<Option<ClientConfig>> {
    load_from(&client_config_path()?)
}

fn builtin_client() -> anyhow::Result<Option<(String, SecretString)>> {
    resolve_pair(
        BUILTIN_CLIENT_ID
            .filter(|v| !v.is_empty())
            .map(str::to_string),
        BUILTIN_CLIENT_SECRET
            .filter(|v| !v.is_empty())
            .map(|v| SecretString::from(v.to_string())),
        "this build of gwsr has only one of GWSR_DEFAULT_CLIENT_ID / GWSR_DEFAULT_CLIENT_SECRET \
         compiled in; rebuild with both or neither",
    )
}

fn resolve_pair(
    id: Option<String>,
    secret: Option<SecretString>,
    mismatch: &str,
) -> anyhow::Result<Option<(String, SecretString)>> {
    match (id, secret) {
        (Some(id), Some(secret)) => Ok(Some((id, secret))),
        (None, None) => Ok(None),
        _ => anyhow::bail!("{mismatch}"),
    }
}

/// Resolve the OAuth client for `gwsr auth login`.
///
/// # Errors
///
/// A half-configured source, an unreadable saved file, or no client at all
/// (with instructions for every way to provide one).
pub fn resolve() -> anyhow::Result<ClientConfig> {
    let path = client_config_path()?;
    resolve_optional()?.ok_or_else(|| anyhow::anyhow!("{}", no_client_message(&path)))
}

/// Like [`resolve`], but `Ok(None)` when no client is configured at all
/// (`gwsr auth status` reports that instead of failing).
///
/// # Errors
///
/// A half-configured source or an unreadable saved file.
pub fn resolve_optional() -> anyhow::Result<Option<ClientConfig>> {
    let path = client_config_path()?;
    let saved = load_from(&path)?;
    let vars = crate::env::get()?;
    let env = resolve_pair(
        vars.client_id.clone(),
        vars.client_secret.clone(),
        "only one of GWSR_CLIENT_ID / GWSR_CLIENT_SECRET is set; set both or neither",
    )?;
    Ok(select(env, saved, builtin_client()?))
}

#[cfg(test)]
fn resolve_from(
    env: Option<(String, SecretString)>,
    saved: Option<ClientConfig>,
    builtin: Option<(String, SecretString)>,
    path: &Path,
) -> anyhow::Result<ClientConfig> {
    select(env, saved, builtin).ok_or_else(|| anyhow::anyhow!("{}", no_client_message(path)))
}

fn select(
    env: Option<(String, SecretString)>,
    saved: Option<ClientConfig>,
    builtin: Option<(String, SecretString)>,
) -> Option<ClientConfig> {
    if let Some((client_id, client_secret)) = env {
        return Some(ClientConfig {
            client_id,
            client_secret,
            project_id: saved.and_then(|s| s.project_id),
            source: ClientSource::EnvVars,
        });
    }
    if let Some(saved) = saved {
        return Some(saved);
    }
    builtin.map(|(client_id, client_secret)| ClientConfig {
        client_id,
        client_secret,
        project_id: None,
        source: ClientSource::BuiltIn,
    })
}

/// Instructions for providing an OAuth client.
pub fn no_client_message(path: &Path) -> String {
    format!(
        "No OAuth client configured. Provide one of:\n  \
         1. `gwsr auth setup` (needs the gcloud CLI) to create a project and client\n  \
         2. In Google Cloud Console, create an OAuth client of type \"Desktop app\" \
         (APIs & Services > Credentials), download its JSON and save it as:\n     \
         {}\n     (then `chmod 600` it)\n  \
         3. Set GWSR_CLIENT_ID and GWSR_CLIENT_SECRET",
        path.display()
    )
}

/// Mask a client ID for display (`12345678...com`).
pub fn mask_client_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() > 12 {
        let head: String = chars[..8].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}...{tail}")
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn save_load_round_trip_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg").join("client_secret.json");
        save_client_config(
            &path,
            "id.apps.googleusercontent.com",
            &secret("GOCSPX-x"),
            Some("proj"),
        )
        .unwrap();
        let loaded = load_from(&path).unwrap().unwrap();
        assert_eq!(loaded.client_id, "id.apps.googleusercontent.com");
        assert_eq!(loaded.client_secret.expose_secret(), "GOCSPX-x");
        assert_eq!(loaded.project_id.as_deref(), Some("proj"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
    }

    #[test]
    fn parses_console_download_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client_secret.json");
        let json = r#"{"installed":{"client_id":"cid","project_id":"p","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","client_secret":"cs","redirect_uris":["http://localhost"]}}"#;
        crate::fs_util::atomic_write(&path, json.as_bytes()).unwrap();
        let c = load_from(&path).unwrap().unwrap();
        assert_eq!(c.client_id, "cid");
        assert_eq!(c.source, ClientSource::SavedFile(path.clone()));

        crate::fs_util::atomic_write(&path, br#"{"web":{}}"#).unwrap();
        assert!(load_from(&path).is_err());
        assert!(
            load_from(&dir.path().join("missing.json"))
                .unwrap()
                .is_none()
        );
    }

    /// A pasted client ID with a trailing space produced `invalid_client`
    /// ("The OAuth client was not found") at login; it must be rejected when
    /// the file is loaded, naming the field.
    #[test]
    fn whitespace_in_client_credentials_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client_secret.json");
        for (json, field) in [
            (
                r#"{"installed":{"client_id":"123-abc.apps.googleusercontent.com ","client_secret":"cs"}}"#,
                "client_id",
            ),
            (
                r#"{"installed":{"client_id":"\t123-abc.apps.googleusercontent.com","client_secret":"cs"}}"#,
                "client_id",
            ),
            (
                r#"{"installed":{"client_id":"123-abc.apps.googleusercontent.com","client_secret":"cs\n"}}"#,
                "client_secret",
            ),
            (
                r#"{"installed":{"client_id":"","client_secret":"cs"}}"#,
                "client_id",
            ),
        ] {
            crate::fs_util::atomic_write(&path, json.as_bytes()).unwrap();
            let err = match load_from(&path) {
                Ok(c) => panic!("accepted {:?} from {json}", c.map(|c| c.client_id)),
                Err(e) => format!("{e:#}"),
            };
            assert!(err.contains(field), "{err}");
            assert!(!err.contains("\"cs"), "secret must not be echoed: {err}");
        }
        assert!(
            save_client_config(&path, "cid ", &secret("cs"), None).is_err(),
            "a dirty client ID must never be written"
        );
    }

    #[cfg(unix)]
    #[test]
    fn world_readable_client_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client_secret.json");
        save_client_config(&path, "cid", &secret("cs"), None).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            load_from(&path)
                .unwrap_err()
                .to_string()
                .contains("chmod 600")
        );
    }

    #[test]
    fn resolution_order_and_errors() {
        let path = PathBuf::from("/cfg/client_secret.json");
        let saved = || ClientConfig {
            client_id: "saved".into(),
            client_secret: secret("s"),
            project_id: Some("proj".into()),
            source: ClientSource::SavedFile(path.clone()),
        };
        let env = || Some(("env".to_string(), secret("e")));
        let builtin = || Some(("builtin".to_string(), secret("b")));

        let c = resolve_from(env(), Some(saved()), builtin(), &path).unwrap();
        assert_eq!(
            (c.client_id.as_str(), c.source),
            ("env", ClientSource::EnvVars)
        );
        assert_eq!(c.project_id.as_deref(), Some("proj"));

        let c = resolve_from(None, Some(saved()), builtin(), &path).unwrap();
        assert_eq!(c.client_id, "saved");

        let c = resolve_from(None, None, builtin(), &path).unwrap();
        assert_eq!(
            (c.client_id.as_str(), c.source),
            ("builtin", ClientSource::BuiltIn)
        );

        let err = resolve_from(None, None, None, &path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("gwsr auth setup"), "{err}");
        assert!(err.contains("Desktop app"), "{err}");
        assert!(err.contains("GWSR_CLIENT_ID"), "{err}");

        assert!(resolve_pair(Some("a".into()), None, "mismatch").is_err());
        assert!(resolve_pair(None, None, "x").unwrap().is_none());
    }

    #[test]
    fn mask() {
        assert_eq!(mask_client_id("1234567890abcdef.apps"), "12345678...apps");
        assert_eq!(mask_client_id("short"), "short");
    }
}
