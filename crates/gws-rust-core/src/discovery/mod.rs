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

//! Discovery Document fetching, caching, and parsing.
//!
//! Google API Discovery Documents describe every resource, method, parameter,
//! and schema of an API; the CLI builds its command tree from them.
//!
//! [`DiscoveryLoader`] fetches documents, validates them (JSON shape, matching
//! name/version, trusted endpoints) **before** caching, serves fresh cached
//! copies, deletes and refetches corrupt cache entries, and falls back to a
//! stale cached copy when the network is unavailable. Every such event is
//! reported to the caller as a [`DiscoveryNotice`] so it can be surfaced.

mod cache;
mod error;
mod model;

use std::time::Duration;

use reqwest::Url;

pub use cache::{CachedDocument, DEFAULT_TTL, DiscoveryCache};
pub use error::DiscoveryError;
pub use model::*;

use crate::validate::validate_api_identifier;

/// Maximum accepted Discovery Document size.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_DISCOVERY_BASE: &str = "https://www.googleapis.com/";

/// Where a loaded document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentOrigin {
    /// Fetched from the network (and cached, if a cache is configured).
    Network,
    /// A fresh cached copy.
    Cache,
    /// An expired cached copy, used because fetching failed.
    StaleCache,
}

/// Something the caller should tell the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiscoveryNotice {
    /// A cached entry was corrupt or untrusted and was ignored (and deleted
    /// when it was a file).
    DiscardedCacheEntry { path: String, reason: String },
    /// The fetched document could not be written to the cache.
    CacheWriteFailed { reason: String },
    /// Fetching failed, so an expired cached copy is being used.
    UsingStaleCache { age: Duration, fetch_error: String },
}

impl std::fmt::Display for DiscoveryNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryNotice::DiscardedCacheEntry { path, reason } => {
                write!(
                    f,
                    "ignored invalid Discovery cache entry '{path}': {reason}"
                )
            }
            DiscoveryNotice::CacheWriteFailed { reason } => {
                write!(f, "could not write Discovery cache: {reason}")
            }
            DiscoveryNotice::UsingStaleCache { age, fetch_error } => write!(
                f,
                "using STALE cached Discovery Document ({} hours old) because fetching failed: {fetch_error}",
                age.as_secs() / 3600
            ),
        }
    }
}

/// A loaded document plus how it was obtained.
#[derive(Debug)]
#[non_exhaustive]
pub struct LoadedDocument {
    pub doc: RestDescription,
    pub origin: DocumentOrigin,
    /// Events the caller should surface (e.g. on stderr).
    pub notices: Vec<DiscoveryNotice>,
}

/// Loads Discovery Documents from the network and an optional cache.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryLoader {
    cache: Option<DiscoveryCache>,
    api_base_override: Option<Url>,
    discovery_base: Option<Url>,
    retry: Option<crate::client::RetryPolicy>,
}

impl DiscoveryLoader {
    /// A loader with no cache and no endpoint override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Use `cache` for reads and writes.
    pub fn with_cache(mut self, cache: DiscoveryCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Trust this API base in addition to `*.googleapis.com` when validating
    /// document endpoints (see [`crate::validate::api_base_override`]).
    pub fn with_api_base_override(mut self, base: Option<Url>) -> Self {
        self.api_base_override = base;
        self
    }

    /// Retry policy for Discovery fetches. Defaults to the process-wide
    /// [`crate::client::RetryPolicy`] with a 60-second response timeout.
    pub fn with_retry_policy(mut self, policy: crate::client::RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    /// The configured cache, if any.
    pub fn cache(&self) -> Option<&DiscoveryCache> {
        self.cache.as_ref()
    }

    /// Remove all cached documents. Returns the number removed (0 without a cache).
    pub fn clear_cache(&self) -> Result<usize, DiscoveryError> {
        match &self.cache {
            Some(cache) => cache.clear(),
            None => Ok(0),
        }
    }

    /// Read a cached document without touching the network.
    ///
    /// Returns `Ok(None)` when nothing is cached (or no cache is configured).
    /// Stale documents are returned. Corrupt or untrusted entries are errors
    /// and are left in place.
    pub fn load_cached(
        &self,
        service: &str,
        version: &str,
    ) -> Result<Option<RestDescription>, DiscoveryError> {
        let (service, version) = validate_ids(service, version)?;
        match &self.cache {
            Some(cache) => Ok(cache
                .load(service, version, self.api_base_override.as_ref())?
                .map(|c| c.doc)),
            None => Ok(None),
        }
    }

    /// Load a document: fresh cache, else network, else stale cache.
    pub async fn load(
        &self,
        service: &str,
        version: &str,
    ) -> Result<LoadedDocument, DiscoveryError> {
        let (service, version) = validate_ids(service, version)?;
        let mut notices = Vec::new();
        let mut stale = None;

        if let Some(cache) = &self.cache {
            let result = {
                let (cache, svc, ver, ovr) = (
                    cache.clone(),
                    service.to_string(),
                    version.to_string(),
                    self.api_base_override.clone(),
                );
                let dir = cache.dir().to_path_buf();
                run_blocking(&dir, move || cache.load(&svc, &ver, ovr.as_ref())).await?
            };
            match result {
                Ok(Some(cached)) if cached.is_fresh => {
                    tracing::debug!(service, version, "Discovery cache hit");
                    return Ok(LoadedDocument {
                        doc: cached.doc,
                        origin: DocumentOrigin::Cache,
                        notices,
                    });
                }
                Ok(Some(cached)) => stale = Some(cached),
                Ok(None) => {}
                Err(DiscoveryError::InsecurePermissions { path, mode }) if path == cache.dir() => {
                    // Ignore every entry; `store` below resets the directory to 0700.
                    notices.push(DiscoveryNotice::DiscardedCacheEntry {
                        path: path.display().to_string(),
                        reason: format!("directory is writable by group or others (mode {mode:o})"),
                    });
                }
                Err(
                    err @ (DiscoveryError::InvalidDocument { .. }
                    | DiscoveryError::InsecurePermissions { .. }),
                ) => {
                    cache.remove(service, version)?;
                    notices.push(DiscoveryNotice::DiscardedCacheEntry {
                        path: cache.entry_path(service, version).display().to_string(),
                        reason: format!("{err} (deleted; refetching)"),
                    });
                }
                Err(other) => return Err(other),
            }
        }

        match self.fetch(service, version).await {
            Ok((doc, body)) => {
                if let Some(cache) = &self.cache {
                    let (cache, svc, ver) =
                        (cache.clone(), service.to_string(), version.to_string());
                    let dir = cache.dir().to_path_buf();
                    let stored = run_blocking(&dir, move || cache.store(&svc, &ver, &body)).await?;
                    if let Err(e) = stored {
                        notices.push(DiscoveryNotice::CacheWriteFailed {
                            reason: e.to_string(),
                        });
                    }
                }
                Ok(LoadedDocument {
                    doc,
                    origin: DocumentOrigin::Network,
                    notices,
                })
            }
            Err(fetch_err) => match stale {
                Some(cached) => {
                    notices.push(DiscoveryNotice::UsingStaleCache {
                        age: cached.age,
                        fetch_error: fetch_err.to_string(),
                    });
                    Ok(LoadedDocument {
                        doc: cached.doc,
                        origin: DocumentOrigin::StaleCache,
                        notices,
                    })
                }
                None => Err(fetch_err),
            },
        }
    }

    /// Candidate URLs for a document: the Discovery directory endpoint, then
    /// the per-service `$discovery/rest` endpoint used by newer APIs.
    fn discovery_urls(&self, service: &str, version: &str) -> Result<Vec<Url>, DiscoveryError> {
        let parse = |s: String| {
            Url::parse(&s)
                .map_err(|e| DiscoveryError::InvalidInput(format!("invalid URL '{s}': {e}")))
        };
        let base = match &self.discovery_base {
            Some(b) => b.as_str().to_string(),
            None => DEFAULT_DISCOVERY_BASE.to_string(),
        };
        // `service` and `version` are validated identifiers ([A-Za-z0-9._-]),
        // which are URL-safe path segments as-is.
        let mut urls = vec![parse(format!(
            "{base}discovery/v1/apis/{service}/{version}/rest"
        ))?];
        if is_dns_label(service) {
            let mut alt = match &self.discovery_base {
                // Tests route the per-service endpoint through the mock server.
                Some(b) => parse(format!("{b}{service}/$discovery/rest"))?,
                None => parse(format!("https://{service}.googleapis.com/$discovery/rest"))?,
            };
            alt.query_pairs_mut().append_pair("version", version);
            urls.push(alt);
        }
        Ok(urls)
    }

    async fn fetch(
        &self,
        service: &str,
        version: &str,
    ) -> Result<(RestDescription, Vec<u8>), DiscoveryError> {
        let fetch_error = |attempts: Vec<String>| DiscoveryError::Fetch {
            service: service.to_string(),
            version: version.to_string(),
            attempts,
        };
        let client =
            crate::client::shared_client().map_err(|e| fetch_error(vec![e.to_string()]))?;
        let policy = self
            .retry
            .clone()
            .unwrap_or_else(|| crate::client::RetryPolicy {
                response_timeout: Some(FETCH_TIMEOUT),
                ..crate::client::RetryPolicy::default()
            });
        let mut attempts = Vec::new();
        let mut all_not_found = true;

        for url in self.discovery_urls(service, version)? {
            tracing::debug!(%url, "Fetching Discovery Document");
            let resp =
                match crate::client::send(&policy, crate::client::Idempotency::FromMethod, || {
                    client.get(url.clone()).timeout(FETCH_TIMEOUT)
                })
                .await
                {
                    Ok(sent) => sent.response,
                    Err(e) => {
                        all_not_found = false;
                        attempts.push(format!("{url}: {}", error_chain(&e)));
                        continue;
                    }
                };
            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                attempts.push(format!("{url}: HTTP 404"));
                continue;
            }
            all_not_found = false;
            if !status.is_success() {
                attempts.push(format!("{url}: HTTP {status}"));
                continue;
            }
            let body = match read_limited(resp).await {
                Ok(body) => body,
                Err(msg) => {
                    attempts.push(format!("{url}: {msg}"));
                    continue;
                }
            };
            match parse_and_validate(
                &body,
                service,
                version,
                self.api_base_override.as_ref(),
                url.as_str(),
            ) {
                Ok(doc) => return Ok((doc, body)),
                Err(e) => attempts.push(e.to_string()),
            }
        }

        if all_not_found {
            Err(DiscoveryError::NotFound {
                service: service.to_string(),
                version: version.to_string(),
            })
        } else {
            Err(fetch_error(attempts))
        }
    }
}

fn validate_ids<'a>(
    service: &'a str,
    version: &'a str,
) -> Result<(&'a str, &'a str), DiscoveryError> {
    let service = validate_api_identifier(service)
        .map_err(|e| DiscoveryError::InvalidInput(format!("Invalid API name: {e}")))?;
    let version = validate_api_identifier(version)
        .map_err(|e| DiscoveryError::InvalidInput(format!("Invalid API version: {e}")))?;
    Ok((service, version))
}

/// Whether `s` can be used as a single DNS label (`[a-z0-9-]`, alnum ends).
fn is_dns_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn error_chain(err: &dyn std::error::Error) -> String {
    let mut msg = err.to_string();
    let mut src = err.source();
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    msg
}

async fn read_limited(mut resp: reqwest::Response) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length()
        && len > MAX_DOCUMENT_BYTES as u64
    {
        return Err(format!("document is too large ({len} bytes)"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| error_chain(&e))? {
        if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err(format!(
                "document exceeds the {MAX_DOCUMENT_BYTES}-byte limit"
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn run_blocking<T: Send + 'static>(
    dir: &std::path::Path,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, DiscoveryError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| DiscoveryError::cache("access", dir, std::io::Error::other(e)))
}

/// Parse a document body and check it is the requested, trusted document.
pub(crate) fn parse_and_validate(
    body: &[u8],
    service: &str,
    version: &str,
    api_base_override: Option<&Url>,
    origin: &str,
) -> Result<RestDescription, DiscoveryError> {
    let invalid = |reason: String| DiscoveryError::InvalidDocument {
        origin: origin.to_string(),
        reason,
    };
    let doc: RestDescription = serde_json::from_slice(body)
        .map_err(|e| invalid(format!("not a Discovery Document: {e}")))?;
    if doc.name != service || doc.version != version {
        return Err(invalid(format!(
            "expected API '{service}' version '{version}', got '{}' version '{}'",
            doc.name.escape_debug(),
            doc.version.escape_debug()
        )));
    }
    doc.validate_endpoints(api_base_override)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(doc)
}

#[cfg(test)]
mod tests;
