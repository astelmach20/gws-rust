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

//! Support for reading `.clasp.json` configuration files.
//!
//! This allows `gws` to be a drop-in replacement for `clasp` by reusing the
//! same project configuration format. When `--script` is omitted, helpers
//! will attempt to read the script ID from `.clasp.json` in the current directory.

use crate::error::GwsError;
use serde::Deserialize;
use std::path::PathBuf;

/// Represents a `.clasp.json` configuration file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaspConfig {
    pub script_id: String,
    pub root_dir: Option<String>,
}

/// Attempts to load `.clasp.json` from the current working directory.
///
/// Returns `Ok(Some(config))` if found and valid, `Ok(None)` if no file exists,
/// or `Err` if the file exists but is malformed or contains unsafe values.
pub fn load_clasp_config() -> Result<Option<ClaspConfig>, GwsError> {
    let path = PathBuf::from(".clasp.json");
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)
        .map_err(|e| GwsError::Validation(format!("Failed to read .clasp.json: {e}")))?;

    let config: ClaspConfig = serde_json::from_str(&contents)
        .map_err(|e| GwsError::Validation(format!("Failed to parse .clasp.json: {e}")))?;

    // Validate scriptId against injection
    if config.script_id.is_empty() {
        return Err(GwsError::Validation(
            ".clasp.json: scriptId must not be empty".to_string(),
        ));
    }
    crate::validate::validate_resource_name(&config.script_id)
        .map_err(|e| GwsError::Validation(format!(".clasp.json: invalid scriptId: {e}")))?;

    // Validate rootDir against path traversal if present
    if let Some(ref root_dir) = config.root_dir {
        if root_dir != "." {
            crate::validate::validate_safe_dir_path(root_dir)
                .map_err(|e| GwsError::Validation(format!(".clasp.json: invalid rootDir: {e}")))?;
        }
    }

    Ok(Some(config))
}

/// Resolves the script ID from an explicit `--script` flag or `.clasp.json`.
///
/// Returns the script ID or an error with a helpful message.
pub fn resolve_script_id(explicit: Option<&String>) -> Result<String, GwsError> {
    if let Some(id) = explicit {
        return Ok(id.clone());
    }

    match load_clasp_config()? {
        Some(config) => Ok(config.script_id),
        None => Err(GwsError::Validation(
            "No --script flag provided and no .clasp.json found in current directory. \
             Either pass --script <ID> or create a .clasp.json with {\"scriptId\": \"...\"}."
                .to_string(),
        )),
    }
}

/// Resolves the working directory from `--dir`, `.clasp.json` `rootDir`, or CWD.
pub fn resolve_dir(explicit_dir: Option<&String>) -> Result<PathBuf, GwsError> {
    if let Some(dir) = explicit_dir {
        return crate::validate::validate_safe_dir_path(dir);
    }

    if let Ok(Some(config)) = load_clasp_config() {
        if let Some(ref root_dir) = config.root_dir {
            return crate::validate::validate_safe_dir_path(root_dir);
        }
    }

    std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn test_clasp_config_parse() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        fs::write(".clasp.json", r#"{"scriptId": "abc123", "rootDir": "."}"#).unwrap();

        let config = load_clasp_config().unwrap().unwrap();
        assert_eq!(config.script_id, "abc123");
        assert_eq!(config.root_dir.as_deref(), Some("."));

        std::env::set_current_dir(&saved_cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_clasp_config_missing() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        let config = load_clasp_config().unwrap();
        assert!(config.is_none());

        std::env::set_current_dir(&saved_cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_clasp_config_no_root_dir() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        fs::write(".clasp.json", r#"{"scriptId": "xyz789"}"#).unwrap();

        let config = load_clasp_config().unwrap().unwrap();
        assert_eq!(config.script_id, "xyz789");
        assert!(config.root_dir.is_none());

        std::env::set_current_dir(&saved_cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_clasp_config_malicious_root_dir() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        fs::write(
            ".clasp.json",
            r#"{"scriptId": "abc", "rootDir": "../../.ssh"}"#,
        )
        .unwrap();

        let result = load_clasp_config();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("rootDir"), "got: {msg}");

        std::env::set_current_dir(&saved_cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_clasp_config_malicious_script_id() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        fs::write(".clasp.json", r#"{"scriptId": "../../../etc/passwd"}"#).unwrap();

        let result = load_clasp_config();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("scriptId"), "got: {msg}");

        std::env::set_current_dir(&saved_cwd).unwrap();
    }

    #[test]
    #[serial]
    fn test_resolve_script_id_explicit() {
        let id = resolve_script_id(Some(&"explicit123".to_string())).unwrap();
        assert_eq!(id, "explicit123");
    }

    #[test]
    #[serial]
    fn test_resolve_script_id_no_config() {
        let dir = tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical).unwrap();

        let result = resolve_script_id(None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains(".clasp.json"), "got: {msg}");

        std::env::set_current_dir(&saved_cwd).unwrap();
    }
}
