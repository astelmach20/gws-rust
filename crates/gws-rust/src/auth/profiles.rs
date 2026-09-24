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

//! Configuration directory layout and named profiles.
//!
//! ```text
//! $GWSR_CONFIG_DIR (default ~/.config/gwsr)      0700
//! ├── client_secret.json                          OAuth client (shared by all profiles)
//! ├── encryption.key                              only with GWSR_KEYRING_BACKEND=file
//! ├── config.toml                                 `profile = "<name>"` (set by `gwsr auth use`)
//! └── profiles/<name>/                            0700
//!     ├── credentials.enc                         encrypted refresh token
//!     ├── token_cache.enc                         encrypted access-token cache
//!     ├── token_cache.lock                        cross-process lock for the cache
//!     └── profile.json                            account + granted scopes (no secrets)
//! ```
//!
//! The active profile is chosen by `--profile`, then `GWSR_PROFILE`, then the
//! `profile` key of `config.toml` (written by `gwsr auth use`), then `default`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::error::GwsError;

/// Name of the profile used when nothing else selects one.
pub const DEFAULT_PROFILE: &str = "default";

const PROFILES_DIR: &str = "profiles";
const MAX_PROFILE_NAME_LEN: usize = 64;

/// Process-wide auth settings taken from global command-line flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalOverrides {
    /// `--profile <name>`
    pub profile: Option<String>,
    /// `--impersonate <user>`
    pub impersonate: Option<String>,
}

static OVERRIDES: OnceLock<GlobalOverrides> = OnceLock::new();

/// Record the global auth flags for this process.
///
/// # Errors
///
/// Fails if the profile name is invalid or the overrides were already set.
pub fn set_global_overrides(overrides: GlobalOverrides) -> Result<(), GwsError> {
    if let Some(name) = &overrides.profile {
        validate_profile_name(name).map_err(|e| GwsError::Validation(format!("{e:#}")))?;
    }
    OVERRIDES
        .set(overrides)
        .map_err(|_| GwsError::Validation("global auth flags were already applied".to_string()))
}

fn overrides() -> Option<&'static GlobalOverrides> {
    OVERRIDES.get()
}

/// Read a string environment variable; unset or empty is `None`.
///
/// # Errors
///
/// The variable is set but not valid UTF-8 (never silently ignored).
pub fn env_string(name: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(name) {
        Ok(v) if v.is_empty() => Ok(None),
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("environment variable {name} is not valid UTF-8")
        }
    }
}

/// Base configuration directory: `GWSR_CONFIG_DIR` or `~/.config/gwsr`.
///
/// # Errors
///
/// Fails if `GWSR_CONFIG_DIR` is unset and the home directory is unknown.
pub fn try_config_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("GWSR_CONFIG_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    let home = dirs::home_dir().context(
        "cannot determine the home directory; set GWSR_CONFIG_DIR to choose a config directory",
    )?;
    Ok(home.join(".config").join("gwsr"))
}

/// Validate a profile name: 1-64 characters of `[A-Za-z0-9_.-]`, not starting
/// with `.` (so it can never be `.`/`..` or a hidden file).
///
/// # Errors
///
/// Returns a message describing the allowed characters.
pub fn validate_profile_name(name: &str) -> anyhow::Result<()> {
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if name.is_empty() || name.len() > MAX_PROFILE_NAME_LEN || !valid_chars || name.starts_with('.')
    {
        anyhow::bail!(
            "invalid profile name '{name}': use 1-{MAX_PROFILE_NAME_LEN} letters, digits, '-', \
             '_' or '.', not starting with '.'"
        );
    }
    Ok(())
}

/// Where the active profile name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    Flag,
    EnvVar,
    ConfigFile,
    Default,
}

/// The profile selected for this invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProfile {
    pub name: String,
    pub source: ProfileSource,
}

/// Resolve the active profile for `base` (flag > env > config.toml > default).
///
/// # Errors
///
/// Fails on an invalid name or an unreadable/invalid `config.toml`.
pub fn active_profile(base: &Path) -> anyhow::Result<ActiveProfile> {
    let flag = overrides().and_then(|o| o.profile.clone());
    let env = env_string("GWSR_PROFILE")?;
    resolve_active_profile(base, flag, env)
}

fn resolve_active_profile(
    base: &Path,
    flag: Option<String>,
    env: Option<String>,
) -> anyhow::Result<ActiveProfile> {
    let (name, source) = if let Some(name) = flag {
        (name, ProfileSource::Flag)
    } else if let Some(name) = env {
        (name, ProfileSource::EnvVar)
    } else if let Some(name) = crate::config::profile_in(&crate::config::config_path_in(base))
        .map_err(|e| anyhow::anyhow!("{e}"))?
    {
        (name, ProfileSource::ConfigFile)
    } else {
        (DEFAULT_PROFILE.to_string(), ProfileSource::Default)
    };
    validate_profile_name(&name).with_context(|| format!("profile selected via {source:?}"))?;
    Ok(ActiveProfile { name, source })
}

/// Persist `name` as the default profile (`profile` in `config.toml`) for
/// future invocations; the rest of the file is preserved.
///
/// # Errors
///
/// Fails on an invalid name, an invalid existing config file or a write error.
pub fn set_active_profile(base: &Path, name: &str) -> anyhow::Result<()> {
    validate_profile_name(name)?;
    crate::fs_util::ensure_private_dir(base)
        .with_context(|| format!("cannot create config directory '{}'", base.display()))?;
    crate::config::set_profile_in(&crate::config::config_path_in(base), name)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// All files belonging to one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePaths {
    pub name: String,
    pub dir: PathBuf,
    pub credentials: PathBuf,
    pub token_cache: PathBuf,
    pub token_lock: PathBuf,
    pub metadata: PathBuf,
}

impl ProfilePaths {
    /// Paths for profile `name` under `base`. The name must already be valid.
    pub fn new(base: &Path, name: &str) -> Self {
        let dir = base.join(PROFILES_DIR).join(name);
        Self {
            name: name.to_string(),
            credentials: dir.join("credentials.enc"),
            token_cache: dir.join("token_cache.enc"),
            token_lock: dir.join("token_cache.lock"),
            metadata: dir.join("profile.json"),
            dir,
        }
    }

    /// Create the profile directory with 0700 permissions.
    ///
    /// # Errors
    ///
    /// Fails if the base or profile directory cannot be created/restricted.
    pub fn ensure_dir(&self) -> anyhow::Result<()> {
        if let Some(profiles) = self.dir.parent() {
            if let Some(base) = profiles.parent() {
                crate::fs_util::ensure_private_dir(base)
                    .with_context(|| format!("cannot create '{}'", base.display()))?;
            }
            crate::fs_util::ensure_private_dir(profiles)
                .with_context(|| format!("cannot create '{}'", profiles.display()))?;
        }
        crate::fs_util::ensure_private_dir(&self.dir)
            .with_context(|| format!("cannot create '{}'", self.dir.display()))
    }
}

/// Paths for the active profile.
///
/// # Errors
///
/// See [`try_config_dir`] and [`active_profile`].
pub fn active_profile_paths() -> anyhow::Result<(PathBuf, ActiveProfile, ProfilePaths)> {
    let base = try_config_dir()?;
    let active = active_profile(&base)?;
    let paths = ProfilePaths::new(&base, &active.name);
    Ok((base, active, paths))
}

/// Names of all profiles that exist on disk, sorted.
///
/// # Errors
///
/// Fails if the profiles directory exists but cannot be read.
pub fn list_profiles(base: &Path) -> anyhow::Result<Vec<String>> {
    let dir = base.join(PROFILES_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read '{}'", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read '{}'", dir.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("cannot stat '{}'", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if validate_profile_name(&name).is_ok() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Non-secret facts about a logged-in profile, stored as `profile.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMetadata {
    /// Email address reported by Google's userinfo endpoint, if known.
    pub account: Option<String>,
    /// OAuth client used for the login.
    pub client_id: String,
    /// Scopes Google reported as granted in the login token response.
    pub granted_scopes: Vec<String>,
    /// RFC 3339 timestamp of the last successful login.
    pub updated_at: String,
}

/// Read `profile.json`, returning `None` if it does not exist.
///
/// # Errors
///
/// Fails if the file exists but cannot be read or parsed.
pub fn load_metadata(path: &Path) -> anyhow::Result<Option<ProfileMetadata>> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .with_context(|| format!("'{}' is not valid profile metadata", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read '{}'", path.display())),
    }
}

/// Write `profile.json` atomically (0600).
///
/// # Errors
///
/// Fails on serialization or write errors.
pub fn save_metadata(path: &Path, meta: &ProfileMetadata) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(meta).context("cannot serialize profile metadata")?;
    crate::fs_util::atomic_write(path, &json)
        .with_context(|| format!("cannot write '{}'", path.display()))
}

/// The service-account subject to impersonate: `--impersonate`, then
/// `GWSR_IMPERSONATE`.
///
/// # Errors
///
/// `GWSR_IMPERSONATE` is not valid UTF-8.
pub fn impersonation_subject() -> anyhow::Result<Option<String>> {
    if let Some(s) = overrides().and_then(|o| o.impersonate.clone()) {
        return Ok(Some(s));
    }
    env_string("GWSR_IMPERSONATE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_validation() {
        for ok in ["default", "work", "a.b-c_d", "X9"] {
            validate_profile_name(ok).unwrap();
        }
        for bad in ["", ".", "..", ".hidden", "a/b", "a b", "ä", &"x".repeat(65)] {
            assert!(validate_profile_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn resolve_profile_priority() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let p = resolve_active_profile(base, None, None).unwrap();
        assert_eq!(p.name, DEFAULT_PROFILE);
        assert_eq!(p.source, ProfileSource::Default);

        // `auth use` writes config.toml and keeps its other keys.
        std::fs::write(base.join("config.toml"), "# mine\nformat = \"table\"\n").unwrap();
        set_active_profile(base, "filed").unwrap();
        let p = resolve_active_profile(base, None, None).unwrap();
        assert_eq!(
            (p.name.as_str(), p.source),
            ("filed", ProfileSource::ConfigFile)
        );
        let text = std::fs::read_to_string(base.join("config.toml")).unwrap();
        assert!(
            text.contains("# mine") && text.contains("format = \"table\""),
            "{text}"
        );
        assert!(!base.join("active_profile").exists());

        let p = resolve_active_profile(base, None, Some("env".into())).unwrap();
        assert_eq!((p.name.as_str(), p.source), ("env", ProfileSource::EnvVar));

        let p = resolve_active_profile(base, Some("flag".into()), Some("env".into())).unwrap();
        assert_eq!((p.name.as_str(), p.source), ("flag", ProfileSource::Flag));

        assert!(resolve_active_profile(base, Some("../x".into()), None).is_err());
    }

    #[test]
    fn invalid_config_profile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "profile = \"../x\"\n").unwrap();
        assert!(resolve_active_profile(dir.path(), None, None).is_err());
        std::fs::write(dir.path().join("config.toml"), "profile = [\n").unwrap();
        assert!(resolve_active_profile(dir.path(), None, None).is_err());
    }

    #[test]
    fn list_profiles_and_metadata_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_profiles(dir.path()).unwrap().is_empty());

        let paths = ProfilePaths::new(dir.path(), "work");
        paths.ensure_dir().unwrap();
        ProfilePaths::new(dir.path(), "home").ensure_dir().unwrap();
        assert_eq!(list_profiles(dir.path()).unwrap(), vec!["home", "work"]);

        assert_eq!(load_metadata(&paths.metadata).unwrap(), None);
        let meta = ProfileMetadata {
            account: Some("a@example.com".into()),
            client_id: "cid".into(),
            granted_scopes: vec!["openid".into()],
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        save_metadata(&paths.metadata, &meta).unwrap();
        assert_eq!(load_metadata(&paths.metadata).unwrap(), Some(meta));

        std::fs::write(&paths.metadata, "not json").unwrap();
        assert!(load_metadata(&paths.metadata).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn profile_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = ProfilePaths::new(dir.path(), "p");
        paths.ensure_dir().unwrap();
        for d in [&paths.dir, paths.dir.parent().unwrap(), dir.path()] {
            let mode = std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", d.display());
        }
    }
}
