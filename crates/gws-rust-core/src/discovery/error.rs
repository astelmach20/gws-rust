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

//! Typed errors for Discovery Document loading.

use std::path::PathBuf;

use crate::error::GwsError;

/// Errors from fetching, caching, or validating Discovery Documents.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// Caller-supplied input (API name, version, configuration) is invalid.
    #[error("{0}")]
    InvalidInput(String),

    /// No Discovery Document exists for this API name and version.
    #[error(
        "No Discovery Document found for API '{service}' version '{version}' (HTTP 404). \
         List available APIs at https://www.googleapis.com/discovery/v1/apis"
    )]
    NotFound { service: String, version: String },

    /// The document could not be fetched.
    #[error(
        "Failed to fetch Discovery Document for '{service}' version '{version}': {}",
        attempts.join("; ")
    )]
    Fetch {
        service: String,
        version: String,
        /// One entry per URL tried, describing why it failed.
        attempts: Vec<String>,
    },

    /// The document was fetched or read but is not a valid, trusted document.
    #[error("Invalid Discovery Document from {origin}: {reason}")]
    InvalidDocument { origin: String, reason: String },

    /// A cache file or directory operation failed.
    #[error("Discovery cache: failed to {action} '{}': {source}", path.display())]
    Cache {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A cache file or directory has permissions that let other users modify it.
    #[error(
        "Discovery cache '{}' is writable by group or others (mode {mode:o}); refusing to trust it",
        path.display()
    )]
    InsecurePermissions { path: PathBuf, mode: u32 },
}

impl DiscoveryError {
    pub(crate) fn cache(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        DiscoveryError::Cache {
            action,
            path: path.into(),
            source,
        }
    }
}

impl From<DiscoveryError> for GwsError {
    fn from(err: DiscoveryError) -> Self {
        match err {
            DiscoveryError::InvalidInput(msg) => GwsError::Validation(msg),
            other => {
                let mut msg = other.to_string();
                let mut src = std::error::Error::source(&other);
                while let Some(s) = src {
                    let text = s.to_string();
                    if !msg.contains(&text) {
                        msg.push_str(": ");
                        msg.push_str(&text);
                    }
                    src = s.source();
                }
                GwsError::Discovery(msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_maps_to_validation() {
        let err: GwsError = DiscoveryError::InvalidInput("bad name".into()).into();
        assert!(matches!(err, GwsError::Validation(ref m) if m == "bad name"));
    }

    #[test]
    fn other_variants_map_to_discovery() {
        let err: GwsError = DiscoveryError::NotFound {
            service: "nope".into(),
            version: "v1".into(),
        }
        .into();
        match err {
            GwsError::Discovery(m) => {
                assert!(m.contains("'nope' version 'v1'"), "{m}");
                assert!(m.contains("404"), "{m}");
            }
            other => panic!("unexpected {other:?}"),
        }
        let err: GwsError =
            DiscoveryError::cache("read", "/x/y.json", std::io::Error::other("boom")).into();
        assert!(err.to_string().contains("boom"), "{err}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_DISCOVERY);
    }
}
