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

//! Discovery Document types and fetching.
//!
//! Types are re-exported from the `gws_rust_core` library crate. This module
//! adds the CLI's configuration (cache location, API base override) and
//! prints loader notices (stale cache, discarded entries) to stderr.

use std::path::PathBuf;

pub use gws_rust_core::discovery::*;

use crate::error::GwsError;

/// Environment variable overriding the cache root (default: the platform
/// cache directory joined with `gwsr`, e.g. `~/Library/Caches/gwsr` or
/// `~/.cache/gwsr`).
pub const CACHE_DIR_ENV: &str = "GWSR_CACHE_DIR";

/// Resolve the cache root from the `GWSR_CACHE_DIR` value and the platform
/// cache directory. The override must be an absolute path.
fn cache_root(
    env_value: Option<std::ffi::OsString>,
    platform_cache: Option<PathBuf>,
) -> Result<PathBuf, GwsError> {
    match env_value.filter(|v| !v.is_empty()) {
        Some(v) => {
            let path = PathBuf::from(v);
            if path.is_absolute() {
                Ok(path)
            } else {
                Err(GwsError::Validation(format!(
                    "{CACHE_DIR_ENV} must be an absolute path, got '{}'",
                    path.display()
                )))
            }
        }
        None => platform_cache.map(|d| d.join("gwsr")).ok_or_else(|| {
            GwsError::Validation(format!(
                "Could not determine the platform cache directory; set {CACHE_DIR_ENV}"
            ))
        }),
    }
}

/// The CLI's Discovery loader: cache under `GWSR_CACHE_DIR` (or the platform
/// cache dir) and endpoint trust extended by `GWSR_API_BASE_URL`.
///
/// Use `loader()?.clear_cache()` for `gwsr cache clear` and
/// `loader()?.load_cached(api, version)` for offline (completion) lookups.
pub fn loader() -> Result<DiscoveryLoader, GwsError> {
    let root = cache_root(std::env::var_os(CACHE_DIR_ENV), dirs::cache_dir())?;
    Ok(DiscoveryLoader::new()
        .with_cache(DiscoveryCache::new(root))
        .with_api_base_override(crate::validate::api_base_override()?))
}

/// Fetches (or reads from cache) a Discovery Document, printing any loader
/// notices — such as the use of a stale cached copy — as warnings on stderr.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
) -> Result<RestDescription, GwsError> {
    let loaded = loader()?.load(service, version).await?;
    for notice in &loaded.notices {
        eprintln!("warning: {notice}");
    }
    Ok(loaded.doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_prefers_absolute_override() {
        let got = cache_root(Some("/custom/cache".into()), Some("/platform".into())).unwrap();
        assert_eq!(got, PathBuf::from("/custom/cache"));
    }

    #[test]
    fn cache_root_defaults_to_platform_dir() {
        let got = cache_root(None, Some("/platform".into())).unwrap();
        assert_eq!(got, PathBuf::from("/platform/gwsr"));
        let got = cache_root(Some("".into()), Some("/platform".into())).unwrap();
        assert_eq!(got, PathBuf::from("/platform/gwsr"));
    }

    #[test]
    fn cache_root_rejects_relative_override_and_missing_platform_dir() {
        assert!(cache_root(Some("relative/dir".into()), None).is_err());
        assert!(cache_root(None, None).is_err());
    }
}
