// Copyright 2026 The gws-rust Authors
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

//! Discovery-cache access for `gwsr cache clear` and shell completion.
//!
//! MERGE NOTE: the core workstream owns the cache layout and provides
//! `discovery::clear_cache() -> Result<usize, _>` and
//! `discovery::load_cached_document(api, version) -> Result<Option<_>, _>`.
//! Until that lands, these functions operate on the current layout
//! (`<config dir>/cache/<api>_<version>.json`); at merge time their bodies are
//! replaced by calls to the core functions with the same contracts.

use std::path::{Path, PathBuf};

use crate::discovery::RestDescription;
use crate::error::GwsError;

fn cache_dir() -> PathBuf {
    crate::auth_commands::config_dir().join("cache")
}

fn io_error(context: String, e: std::io::Error) -> GwsError {
    GwsError::from(anyhow::Error::new(e).context(context))
}

/// Delete every cached Discovery document. Returns how many were removed.
pub fn clear_cache() -> Result<usize, GwsError> {
    clear_dir(&cache_dir())
}

fn clear_dir(dir: &Path) -> Result<usize, GwsError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(io_error(format!("failed to read {}", dir.display()), e)),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| io_error(format!("failed to read {}", dir.display()), e))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            std::fs::remove_file(&path)
                .map_err(|e| io_error(format!("failed to remove {}", path.display()), e))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Read a cached Discovery document without touching the network.
/// `Ok(None)` when nothing is cached; stale documents are returned.
pub fn load_cached_document(
    api_name: &str,
    version: &str,
) -> Result<Option<RestDescription>, GwsError> {
    let api_name = crate::validate::validate_api_identifier(api_name)?;
    let version = crate::validate::validate_api_identifier(version)?;
    load_from(&cache_dir().join(format!("{api_name}_{version}.json")))
}

fn load_from(path: &Path) -> Result<Option<RestDescription>, GwsError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(format!("failed to read {}", path.display()), e)),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| {
        GwsError::Discovery(format!("corrupt cached document {}: {e}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_removes_only_json_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("drive_v3.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("gmail_v1.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("keep.txt"), "x").unwrap();
        assert_eq!(clear_dir(tmp.path()).unwrap(), 2);
        assert!(tmp.path().join("keep.txt").exists());
        assert_eq!(clear_dir(&tmp.path().join("missing")).unwrap(), 0);
    }

    #[test]
    fn load_from_missing_corrupt_and_valid() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_from(&tmp.path().join("x.json")).unwrap().is_none());
        let bad = tmp.path().join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        assert!(load_from(&bad).is_err());
        let good = tmp.path().join("good.json");
        std::fs::write(&good, r#"{"name":"drive","version":"v3","rootUrl":"https://www.googleapis.com/","servicePath":"drive/v3/"}"#).unwrap();
        assert_eq!(load_from(&good).unwrap().unwrap().name, "drive");
    }
}
