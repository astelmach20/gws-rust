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
//! ├── active_profile                              name of the profile used by default
//! └── profiles/<name>/                            0700
//!     ├── credentials.enc                         encrypted refresh token
//!     ├── token_cache.enc                         encrypted access-token cache
//!     ├── token_cache.lock                        cross-process lock for the cache
//!     └── profile.json                            account + granted scopes (no secrets)
//! ```
//!
//! The active profile is chosen by `--profile`, then `GWSR_PROFILE`, then the
//! `active_profile` file, then `default`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::error::GwsError;

/// Name of the profile used when nothing else selects one.
pub const DEFAULT_PROFILE: &str = "default";

const PROFILES_DIR: &str = "profiles";
const ACTIVE_PROFILE_FILE: &str = "active_profile";
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

/// Extract `--profile` and `--impersonate` (both `--flag value` and
/// `--flag=value` forms) from `args`, removing them so the rest of the CLI
/// never sees them, and record them for this process.
///
/// Scanning stops at a literal `--`. `args[0]` (the program name) is skipped.
///
/// # Errors
///
/// Fails on a flag without a value, a repeated flag, or an invalid profile name.
pub fn apply_global_flags(args: &mut Vec<String>) -> Result<(), GwsError> {
    let overrides = extract_global_flags(args)?;
    set_global_overrides(overrides)
}

pub(crate) fn extract_global_flags(args: &mut Vec<String>) -> Result<GlobalOverrides, GwsError> {
    let mut out = GlobalOverrides::default();
    let mut kept = Vec::with_capacity(args.len());
    let mut iter = std::mem::take(args).into_iter();
    if let Some(program) = iter.next() {
        kept.push(program);
    }
    while let Some(arg) = iter.next() {
        if arg == "--" {
            kept.push(arg);
            kept.extend(iter.by_ref());
            break;
        }
        let (flag, inline_value) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let slot = match flag.as_str() {
            "--profile" => &mut out.profile,
            "--impersonate" => &mut out.impersonate,
            _ => {
                kept.push(arg);
                continue;
            }
        };
        let value = match inline_value {
            Some(v) => v,
            None => iter
                .next()
                .ok_or_else(|| GwsError::Validation(format!("{flag} requires a value")))?,
        };
        if value.is_empty() || value.starts_with("--") {
            return Err(GwsError::Validation(format!(
                "{flag} requires a non-empty value"
            )));
        }
        if slot.is_some() {
            return Err(GwsError::Validation(format!(
                "{flag} was given more than once"
            )));
        }
        *slot = Some(value);
    }
    *args = kept;
    Ok(out)
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

/// Infallible form of [`try_config_dir`] for callers that only use the
/// directory for caches. If the home directory is unknown it warns loudly and
/// uses `./.gwsr`.
pub fn config_dir() -> PathBuf {
    match try_config_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("warning: {e:#}; using ./.gwsr for cache files");
            PathBuf::from(".gwsr")
        }
    }
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
    ActiveProfileFile,
    Default,
}

/// The profile selected for this invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProfile {
    pub name: String,
    pub source: ProfileSource,
}

/// Resolve the active profile for `base` (flag > env > file > default).
///
/// # Errors
///
/// Fails on an invalid name or an unreadable `active_profile` file.
pub fn active_profile(base: &Path) -> anyhow::Result<ActiveProfile> {
    let flag = overrides().and_then(|o| o.profile.clone());
    let env = std::env::var("GWSR_PROFILE").ok().filter(|v| !v.is_empty());
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
    } else if let Some(name) = read_active_profile_file(base)? {
        (name, ProfileSource::ActiveProfileFile)
    } else {
        (DEFAULT_PROFILE.to_string(), ProfileSource::Default)
    };
    validate_profile_name(&name).with_context(|| format!("profile selected via {source:?}"))?;
    Ok(ActiveProfile { name, source })
}

fn read_active_profile_file(base: &Path) -> anyhow::Result<Option<String>> {
    let path = base.join(ACTIVE_PROFILE_FILE);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let name = s.trim().to_string();
            if name.is_empty() {
                anyhow::bail!(
                    "'{}' is empty; run `gwsr auth use <profile>` to select a profile",
                    path.display()
                );
            }
            Ok(Some(name))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read '{}'", path.display())),
    }
}

/// Persist `name` as the default profile for future invocations.
///
/// # Errors
///
/// Fails on an invalid name or a write error.
pub fn set_active_profile(base: &Path, name: &str) -> anyhow::Result<()> {
    validate_profile_name(name)?;
    crate::fs_util::ensure_private_dir(base)
        .with_context(|| format!("cannot create config directory '{}'", base.display()))?;
    let path = base.join(ACTIVE_PROFILE_FILE);
    crate::fs_util::atomic_write(&path, format!("{name}\n").as_bytes())
        .with_context(|| format!("cannot write '{}'", path.display()))
}

/// Whether an `active_profile` file exists.
pub fn has_active_profile_file(base: &Path) -> bool {
    base.join(ACTIVE_PROFILE_FILE).is_file()
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
pub fn impersonation_subject() -> Option<String> {
    overrides()
        .and_then(|o| o.impersonate.clone())
        .or_else(|| std::env::var("GWSR_IMPERSONATE").ok())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extract_global_flags_both_forms() {
        let mut a = args(&[
            "gwsr",
            "--profile",
            "work",
            "drive",
            "files",
            "list",
            "--impersonate=alice@example.com",
        ]);
        let o = extract_global_flags(&mut a).unwrap();
        assert_eq!(o.profile.as_deref(), Some("work"));
        assert_eq!(o.impersonate.as_deref(), Some("alice@example.com"));
        assert_eq!(a, args(&["gwsr", "drive", "files", "list"]));
    }

    #[test]
    fn extract_global_flags_stops_at_double_dash() {
        let mut a = args(&["gwsr", "x", "--", "--profile", "p"]);
        let o = extract_global_flags(&mut a).unwrap();
        assert_eq!(o, GlobalOverrides::default());
        assert_eq!(a, args(&["gwsr", "x", "--", "--profile", "p"]));
    }

    #[test]
    fn extract_global_flags_errors() {
        assert!(extract_global_flags(&mut args(&["gwsr", "--profile"])).is_err());
        assert!(extract_global_flags(&mut args(&["gwsr", "--profile="])).is_err());
        assert!(extract_global_flags(&mut args(&["gwsr", "--profile", "--x"])).is_err());
        assert!(
            extract_global_flags(&mut args(&["gwsr", "--profile", "a", "--profile", "b"])).is_err()
        );
    }

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

        set_active_profile(base, "filed").unwrap();
        let p = resolve_active_profile(base, None, None).unwrap();
        assert_eq!(
            (p.name.as_str(), p.source),
            ("filed", ProfileSource::ActiveProfileFile)
        );

        let p = resolve_active_profile(base, None, Some("env".into())).unwrap();
        assert_eq!((p.name.as_str(), p.source), ("env", ProfileSource::EnvVar));

        let p = resolve_active_profile(base, Some("flag".into()), Some("env".into())).unwrap();
        assert_eq!((p.name.as_str(), p.source), ("flag", ProfileSource::Flag));

        assert!(resolve_active_profile(base, Some("../x".into()), None).is_err());
    }

    #[test]
    fn empty_active_profile_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ACTIVE_PROFILE_FILE), "\n").unwrap();
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
