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

//! File-system path validation.
//!
//! By default any path is accepted: absolute paths, `~`-expanded paths from
//! the shell, and paths outside the current directory. Sandboxed agents can
//! opt into a strict mode with `GWSR_RESTRICT_PATHS=cwd`, which requires every
//! resolved path (after following symlinks) to stay under the current
//! working directory.
//!
//! In every mode, paths containing control characters or dangerous Unicode
//! are rejected, and the returned path is absolute with symlinks in its
//! existing prefix resolved, so the validated path is the one used for I/O.
//!
//! # TOCTOU caveat
//!
//! This is a best-effort check. A local attacker with write access to a
//! parent directory could replace a path component between validation and
//! the subsequent I/O.

use std::path::{Component, Path, PathBuf};

use super::reject_dangerous_chars;
use crate::error::GwsError;

/// Environment variable selecting the [`PathPolicy`].
pub const RESTRICT_PATHS_ENV: &str = "GWSR_RESTRICT_PATHS";

/// Where file-system paths given on the command line may point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PathPolicy {
    /// Any path is allowed (default).
    #[default]
    Unrestricted,
    /// Resolved paths must stay under the current working directory
    /// (`GWSR_RESTRICT_PATHS=cwd`).
    Cwd,
}

impl PathPolicy {
    /// Parse a `GWSR_RESTRICT_PATHS` value. Unset or empty means
    /// [`PathPolicy::Unrestricted`]; `cwd` means [`PathPolicy::Cwd`]; anything
    /// else is an error so a typo never silently disables the sandbox.
    pub fn parse(value: Option<&str>) -> Result<Self, GwsError> {
        match value.map(str::trim) {
            None | Some("") => Ok(PathPolicy::Unrestricted),
            Some(v) if v.eq_ignore_ascii_case("cwd") => Ok(PathPolicy::Cwd),
            Some(other) => Err(GwsError::Validation(format!(
                "Invalid {RESTRICT_PATHS_ENV} value '{}': expected 'cwd' or unset",
                other.escape_debug()
            ))),
        }
    }

    /// Read the policy from `GWSR_RESTRICT_PATHS`.
    pub fn from_env() -> Result<Self, GwsError> {
        match std::env::var(RESTRICT_PATHS_ENV) {
            Ok(v) => Self::parse(Some(&v)),
            Err(std::env::VarError::NotPresent) => Ok(PathPolicy::Unrestricted),
            Err(std::env::VarError::NotUnicode(_)) => Err(GwsError::Validation(format!(
                "{RESTRICT_PATHS_ENV} is not valid UTF-8"
            ))),
        }
    }
}

fn current_dir() -> Result<PathBuf, GwsError> {
    std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))
}

/// Validates a file path such as `--upload` or `--output` using the policy
/// from `GWSR_RESTRICT_PATHS`. See [`resolve_file_path`].
pub fn validate_safe_file_path(path_str: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    resolve_file_path(
        path_str,
        flag_name,
        PathPolicy::from_env()?,
        &current_dir()?,
    )
}

/// Validates an output directory (e.g. `--output-dir`) using the policy from
/// `GWSR_RESTRICT_PATHS`. See [`resolve_output_dir`].
pub fn validate_safe_output_dir(dir: &str) -> Result<PathBuf, GwsError> {
    resolve_output_dir(dir, PathPolicy::from_env()?, &current_dir()?)
}

/// Validates an existing directory to read from (e.g. `--dir`) using the
/// policy from `GWSR_RESTRICT_PATHS`. See [`resolve_dir_path`].
pub fn validate_safe_dir_path(dir: &str) -> Result<PathBuf, GwsError> {
    resolve_dir_path(dir, PathPolicy::from_env()?, &current_dir()?)
}

/// Resolves a file path (which may not exist yet) relative to `cwd`.
///
/// Returns an absolute path whose existing prefix is canonicalized (symlinks
/// resolved) and whose `.`/`..` components are normalized. Under
/// [`PathPolicy::Cwd`] the result must lie under `cwd`.
pub fn resolve_file_path(
    path_str: &str,
    flag_name: &str,
    policy: PathPolicy,
    cwd: &Path,
) -> Result<PathBuf, GwsError> {
    let resolved = resolve_any(path_str, flag_name, cwd)?;
    enforce_policy(&resolved, path_str, flag_name, policy, cwd)?;
    Ok(resolved)
}

/// Resolves an output directory (which may not exist yet) relative to `cwd`.
pub fn resolve_output_dir(dir: &str, policy: PathPolicy, cwd: &Path) -> Result<PathBuf, GwsError> {
    resolve_file_path(dir, "--output-dir", policy, cwd)
}

/// Resolves an existing directory relative to `cwd`.
pub fn resolve_dir_path(dir: &str, policy: PathPolicy, cwd: &Path) -> Result<PathBuf, GwsError> {
    let resolved = resolve_any(dir, "--dir", cwd)?;
    if !resolved.is_dir() {
        return Err(GwsError::Validation(format!(
            "--dir '{dir}' is not an existing directory"
        )));
    }
    enforce_policy(&resolved, dir, "--dir", policy, cwd)?;
    Ok(resolved)
}

fn resolve_any(path_str: &str, flag_name: &str, cwd: &Path) -> Result<PathBuf, GwsError> {
    if path_str.is_empty() {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not be empty"
        )));
    }
    reject_dangerous_chars(path_str, flag_name)?;
    let joined = cwd.join(path_str);
    let normalized = normalize_existing_prefix(&joined).map_err(|e| {
        GwsError::Validation(format!("Failed to resolve {flag_name} '{path_str}': {e}"))
    })?;
    Ok(normalize_dotdot(&normalized))
}

fn enforce_policy(
    resolved: &Path,
    path_str: &str,
    flag_name: &str,
    policy: PathPolicy,
    cwd: &Path,
) -> Result<(), GwsError> {
    match policy {
        PathPolicy::Unrestricted => Ok(()),
        PathPolicy::Cwd => {
            let canonical_cwd = cwd.canonicalize().map_err(|e| {
                GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
            })?;
            if resolved.starts_with(&canonical_cwd) {
                Ok(())
            } else {
                Err(GwsError::Validation(format!(
                    "{flag_name} '{path_str}' resolves to '{}' which is outside the current directory ({RESTRICT_PATHS_ENV}=cwd is set)",
                    resolved.display()
                )))
            }
        }
    }
}

/// Resolve `.` and `..` components lexically, without touching the filesystem.
fn normalize_dotdot(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Canonicalizes the longest existing prefix of `path` (resolving symlinks)
/// and appends the remaining, not-yet-existing components.
///
/// A path component that is a symlink counts as existing even when it is
/// dangling, so canonicalization fails loudly instead of letting later I/O
/// follow the link to an unchecked location.
fn normalize_existing_prefix(path: &Path) -> std::io::Result<PathBuf> {
    let mut remaining = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if current.symlink_metadata().is_ok() {
            let mut resolved = current.canonicalize()?;
            for seg in remaining.into_iter().rev() {
                resolved.push(seg);
            }
            return Ok(resolved);
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                remaining.push(name.to_os_string());
                current = parent.to_path_buf();
            }
            _ => {
                // `..` or root without an existing prefix: normalize lexically.
                return Ok(path.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn canon_tmp() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        (dir, canonical)
    }

    #[test]
    fn policy_parse() {
        assert_eq!(PathPolicy::parse(None).unwrap(), PathPolicy::Unrestricted);
        assert_eq!(
            PathPolicy::parse(Some("")).unwrap(),
            PathPolicy::Unrestricted
        );
        assert_eq!(PathPolicy::parse(Some("cwd")).unwrap(), PathPolicy::Cwd);
        assert_eq!(PathPolicy::parse(Some(" CWD ")).unwrap(), PathPolicy::Cwd);
        let err = PathPolicy::parse(Some("cdw")).unwrap_err().to_string();
        assert!(err.contains("GWSR_RESTRICT_PATHS"), "{err}");
    }

    #[test]
    fn default_allows_absolute_paths_outside_cwd() {
        let (_d1, cwd) = canon_tmp();
        let (_d2, other) = canon_tmp();
        let target = other.join("out.pdf");
        let got = resolve_file_path(
            target.to_str().unwrap(),
            "--output",
            PathPolicy::Unrestricted,
            &cwd,
        )
        .unwrap();
        assert_eq!(got, target);
    }

    #[test]
    fn default_allows_parent_relative_paths() {
        let (_d, root) = canon_tmp();
        let cwd = root.join("work");
        fs::create_dir(&cwd).unwrap();
        fs::write(root.join("up.txt"), "x").unwrap();
        let got =
            resolve_file_path("../up.txt", "--upload", PathPolicy::Unrestricted, &cwd).unwrap();
        assert_eq!(got, root.join("up.txt"));
    }

    #[test]
    fn relative_path_resolves_under_cwd() {
        let (_d, cwd) = canon_tmp();
        fs::write(cwd.join("test.txt"), "data").unwrap();
        for policy in [PathPolicy::Unrestricted, PathPolicy::Cwd] {
            let got = resolve_file_path("test.txt", "--upload", policy, &cwd).unwrap();
            assert_eq!(got, cwd.join("test.txt"));
        }
    }

    #[test]
    fn non_existing_nested_path_is_normalized() {
        let (_d, cwd) = canon_tmp();
        let got = resolve_file_path("new/./a/../b.txt", "--output", PathPolicy::Cwd, &cwd).unwrap();
        assert_eq!(got, cwd.join("new/b.txt"));
    }

    #[test]
    fn strict_rejects_traversal() {
        let (_d, cwd) = canon_tmp();
        let err = resolve_file_path("../../etc/passwd", "--upload", PathPolicy::Cwd, &cwd)
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside the current directory"), "{err}");
        assert!(err.contains("GWSR_RESTRICT_PATHS=cwd"), "{err}");
    }

    #[test]
    fn strict_rejects_traversal_via_nonexistent_prefix() {
        let (_d, cwd) = canon_tmp();
        assert!(
            resolve_file_path(
                "doesnt_exist/../../etc/passwd",
                "--output",
                PathPolicy::Cwd,
                &cwd
            )
            .is_err()
        );
    }

    #[test]
    fn strict_rejects_absolute_outside() {
        let (_d, cwd) = canon_tmp();
        assert!(resolve_file_path("/etc/hosts", "--upload", PathPolicy::Cwd, &cwd).is_err());
        assert!(resolve_output_dir("/tmp/evil", PathPolicy::Cwd, &cwd).is_err());
    }

    #[test]
    fn strict_allows_absolute_inside() {
        let (_d, cwd) = canon_tmp();
        let inside = cwd.join("x.txt");
        assert!(
            resolve_file_path(inside.to_str().unwrap(), "--output", PathPolicy::Cwd, &cwd).is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn strict_rejects_symlink_escape() {
        let (_d, cwd) = canon_tmp();
        let (_o, outside) = canon_tmp();
        std::os::unix::fs::symlink(&outside, cwd.join("escape")).unwrap();
        assert!(resolve_file_path("escape/secret.txt", "--output", PathPolicy::Cwd, &cwd).is_err());
        assert!(resolve_output_dir("escape", PathPolicy::Cwd, &cwd).is_err());
        assert!(resolve_dir_path("escape", PathPolicy::Cwd, &cwd).is_err());
        // Unrestricted mode follows the link and returns the real target.
        let got = resolve_file_path(
            "escape/secret.txt",
            "--output",
            PathPolicy::Unrestricted,
            &cwd,
        )
        .unwrap();
        assert_eq!(got, outside.join("secret.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_fails_loudly() {
        let (_d, cwd) = canon_tmp();
        std::os::unix::fs::symlink(cwd.join("missing-target"), cwd.join("dangling")).unwrap();
        for policy in [PathPolicy::Unrestricted, PathPolicy::Cwd] {
            let err = resolve_file_path("dangling", "--output", policy, &cwd)
                .unwrap_err()
                .to_string();
            assert!(err.contains("Failed to resolve --output"), "{err}");
        }
    }

    #[test]
    fn rejects_control_and_dangerous_chars() {
        let (_d, cwd) = canon_tmp();
        for bad in [
            "file\0.txt",
            "foo\x01bar",
            "foo\u{200B}bar",
            "foo\u{202E}bar",
            "a\u{2028}b",
        ] {
            for policy in [PathPolicy::Unrestricted, PathPolicy::Cwd] {
                assert!(
                    resolve_file_path(bad, "--output", policy, &cwd).is_err(),
                    "{bad:?}"
                );
                assert!(resolve_output_dir(bad, policy, &cwd).is_err(), "{bad:?}");
            }
        }
    }

    #[test]
    fn rejects_empty() {
        let (_d, cwd) = canon_tmp();
        assert!(resolve_file_path("", "--output", PathPolicy::Unrestricted, &cwd).is_err());
    }

    #[test]
    fn dir_path_requires_existing_directory() {
        let (_d, cwd) = canon_tmp();
        fs::create_dir(cwd.join("src")).unwrap();
        fs::write(cwd.join("file"), "x").unwrap();
        assert_eq!(
            resolve_dir_path(".", PathPolicy::Cwd, &cwd).unwrap(),
            cwd.clone()
        );
        assert_eq!(
            resolve_dir_path("src", PathPolicy::Cwd, &cwd).unwrap(),
            cwd.join("src")
        );
        assert!(resolve_dir_path("missing", PathPolicy::Unrestricted, &cwd).is_err());
        assert!(resolve_dir_path("file", PathPolicy::Unrestricted, &cwd).is_err());
        assert!(resolve_dir_path("..", PathPolicy::Cwd, &cwd).is_err());
        assert!(resolve_dir_path("..", PathPolicy::Unrestricted, &cwd).is_ok());
    }

    #[test]
    fn output_dir_non_existing_subdir() {
        let (_d, cwd) = canon_tmp();
        assert_eq!(
            resolve_output_dir("new/nested/dir", PathPolicy::Cwd, &cwd).unwrap(),
            cwd.join("new/nested/dir")
        );
    }

    #[test]
    fn env_wrappers_use_current_dir() {
        // Only meaningful when the test environment does not set the policy.
        if std::env::var_os(RESTRICT_PATHS_ENV).is_none() {
            assert!(validate_safe_file_path("/definitely/absolute/out.txt", "--output").is_ok());
            assert!(validate_safe_dir_path(".").is_ok());
        }
    }
}
