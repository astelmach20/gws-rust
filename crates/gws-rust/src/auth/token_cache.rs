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

//! Encrypted, cross-process-safe access-token cache for a profile.
//!
//! The whole read → refresh → write sequence runs while holding an exclusive
//! lock on `token_cache.lock`, so concurrent `gwsr` processes never lose each
//! other's updates and only one of them refreshes an expired token.
//!
//! The cache only ever holds short-lived access tokens. If it cannot be
//! decrypted even though the profile's credentials could (same key), it is
//! corrupt: it is renamed to `token_cache.enc.corrupt-<unix time>` with a
//! warning and rebuilt. It is never deleted.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::AuthError;
use super::keystore::{Keystore, KeystoreError, Purpose};
use super::token::AccessToken;

const CACHE_VERSION: u32 = 1;
/// Tokens this close to expiry are treated as expired.
const EXPIRY_MARGIN_SECS: i64 = 60;
const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    entries: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    access_token: String,
    expires_at: i64,
}

impl Drop for Entry {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.access_token);
    }
}

impl Default for CacheFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// A profile's token cache.
/// Whether a cached token may be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Reuse an unexpired cached token.
    Cached,
    /// Skip the cache and mint a new token (after the server rejected the
    /// cached one with HTTP 401); the new token replaces the cached entry.
    ForceRefresh,
}

pub struct TokenCache {
    path: PathBuf,
    lock_path: PathBuf,
    keystore: Arc<Keystore>,
}

impl TokenCache {
    pub fn new(path: &Path, lock_path: &Path, keystore: Arc<Keystore>) -> Self {
        Self {
            path: path.to_path_buf(),
            lock_path: lock_path.to_path_buf(),
            keystore,
        }
    }

    /// Return a valid cached token for `key`, or obtain one with `fetch`,
    /// store it, and return it. Runs under the cross-process lock.
    ///
    /// # Errors
    ///
    /// Lock timeouts, key/IO errors, and errors from `fetch`.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        key: &str,
        freshness: Freshness,
        fetch: F,
    ) -> Result<SecretString, AuthError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<AccessToken, AuthError>>,
    {
        let _lock = crate::fs_util::FileLock::acquire_async(&self.lock_path, LOCK_TIMEOUT)
            .await
            .map_err(|e| {
                AuthError::Storage(format!(
                    "cannot lock token cache '{}': {e}",
                    self.lock_path.display()
                ))
            })?;

        let mut cache = self.read_blocking().await?;
        let now = chrono::Utc::now().timestamp();
        if freshness == Freshness::Cached
            && let Some(entry) = cache.entries.get(key)
            && entry.expires_at - EXPIRY_MARGIN_SECS > now
        {
            tracing::debug!(key, "using cached access token");
            return Ok(SecretString::from(entry.access_token.clone()));
        }

        let fresh = fetch().await?;
        match fresh.expires_at {
            Some(expires_at) => {
                cache.entries.retain(|_, e| e.expires_at > now);
                cache.entries.insert(
                    key.to_string(),
                    Entry {
                        access_token: fresh.token.expose_secret().to_string(),
                        expires_at,
                    },
                );
                self.write_blocking(cache).await?;
            }
            None => tracing::debug!(key, "token has no expiry; not caching it"),
        }
        Ok(fresh.token)
    }

    async fn read_blocking(&self) -> Result<CacheFile, AuthError> {
        let path = self.path.clone();
        let keystore = Arc::clone(&self.keystore);
        tokio::task::spawn_blocking(move || read_cache(&path, &keystore))
            .await
            .map_err(|e| AuthError::Storage(format!("token cache task failed: {e}")))?
    }

    async fn write_blocking(&self, cache: CacheFile) -> Result<(), AuthError> {
        let path = self.path.clone();
        let keystore = Arc::clone(&self.keystore);
        tokio::task::spawn_blocking(move || write_cache(&path, &keystore, &cache))
            .await
            .map_err(|e| AuthError::Storage(format!("token cache task failed: {e}")))?
    }
}

fn read_cache(path: &Path, keystore: &Keystore) -> Result<CacheFile, AuthError> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CacheFile::default()),
        Err(e) => {
            return Err(AuthError::Storage(format!(
                "cannot read token cache '{}': {e}",
                path.display()
            )));
        }
    };
    let what = format!("the token cache '{}'", path.display());
    let parsed = match keystore.decrypt(Purpose::TokenCache, &data, &what) {
        Ok(plaintext) => serde_json::from_slice::<CacheFile>(&plaintext)
            .map_err(|e| format!("invalid JSON: {e}"))
            .and_then(|c| {
                if c.version == CACHE_VERSION {
                    Ok(c)
                } else {
                    Err(format!("unsupported cache version {}", c.version))
                }
            }),
        Err(e @ (KeystoreError::Decrypt { .. } | KeystoreError::UnsupportedFormat { .. })) => {
            Err(e.to_string())
        }
        Err(e) => return Err(e.into()),
    };
    match parsed {
        Ok(cache) => Ok(cache),
        Err(reason) => {
            let quarantine = quarantine_path(path);
            std::fs::rename(path, &quarantine).map_err(|e| {
                AuthError::Storage(format!(
                    "the token cache '{}' is unreadable ({reason}) and could not be moved aside: \
                     {e}",
                    path.display()
                ))
            })?;
            eprintln!(
                "warning: the token cache '{}' was unreadable ({reason}); moved it to '{}' and \
                 starting a new cache (it only holds short-lived access tokens)",
                path.display(),
                quarantine.display()
            );
            Ok(CacheFile::default())
        }
    }
}

fn quarantine_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".corrupt-{}", chrono::Utc::now().timestamp()));
    path.with_file_name(name)
}

fn write_cache(path: &Path, keystore: &Keystore, cache: &CacheFile) -> Result<(), AuthError> {
    let json = Zeroizing::new(
        serde_json::to_vec(cache)
            .map_err(|e| AuthError::Storage(format!("cannot serialize token cache: {e}")))?,
    );
    let ct = keystore.encrypt(Purpose::TokenCache, &json)?;
    crate::fs_util::atomic_write(path, &ct).map_err(|e| {
        AuthError::Storage(format!(
            "cannot write token cache '{}': {e}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::super::keystore::testing::memory_keystore;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn token(value: &str, ttl: i64) -> AccessToken {
        AccessToken {
            token: SecretString::from(value.to_string()),
            expires_at: Some(chrono::Utc::now().timestamp() + ttl),
            scopes: None,
        }
    }

    fn cache_in(dir: &Path) -> TokenCache {
        let (ks, _) = memory_keystore(dir);
        TokenCache::new(
            &dir.join("token_cache.enc"),
            &dir.join("token_cache.lock"),
            Arc::new(ks),
        )
    }

    #[tokio::test]
    async fn caches_until_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        let calls = AtomicUsize::new(0);
        for _ in 0..3 {
            let t = cache
                .get_or_fetch("k", Freshness::Cached, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(token("ya29.a", 3600))
                })
                .await
                .unwrap();
            assert_eq!(t.expose_secret(), "ya29.a");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Nearly-expired tokens are refreshed.
        let t = cache
            .get_or_fetch("short", Freshness::Cached, || async {
                Ok(token("ya29.s", 30))
            })
            .await
            .unwrap();
        assert_eq!(t.expose_secret(), "ya29.s");
        let t = cache
            .get_or_fetch("short", Freshness::Cached, || async {
                Ok(token("ya29.s2", 3600))
            })
            .await
            .unwrap();
        assert_eq!(t.expose_secret(), "ya29.s2");
    }

    #[tokio::test]
    async fn force_refresh_bypasses_and_replaces_the_cached_token() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        cache
            .get_or_fetch("k", Freshness::Cached, || async {
                Ok(token("ya29.old", 3600))
            })
            .await
            .unwrap();
        let t = cache
            .get_or_fetch("k", Freshness::ForceRefresh, || async {
                Ok(token("ya29.new", 3600))
            })
            .await
            .unwrap();
        assert_eq!(t.expose_secret(), "ya29.new");
        let t = cache
            .get_or_fetch("k", Freshness::Cached, || async {
                Err(AuthError::Network("must not be called".into()))
            })
            .await
            .unwrap();
        assert_eq!(t.expose_secret(), "ya29.new");
    }

    #[tokio::test]
    async fn file_is_encrypted_at_rest() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        cache
            .get_or_fetch("k", Freshness::Cached, || async {
                Ok(token("ya29.secret-token", 3600))
            })
            .await
            .unwrap();
        let raw = std::fs::read(dir.path().join("token_cache.enc")).unwrap();
        assert_eq!(&raw[..4], b"GWSR");
        assert!(!String::from_utf8_lossy(&raw).contains("ya29.secret-token"));
    }

    #[tokio::test]
    async fn fetch_errors_propagate_and_leave_cache_intact() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        cache
            .get_or_fetch("a", Freshness::Cached, || async {
                Ok(token("ya29.a", 3600))
            })
            .await
            .unwrap();
        let before = std::fs::read(dir.path().join("token_cache.enc")).unwrap();
        let err = cache
            .get_or_fetch("b", Freshness::Cached, || async {
                Err(AuthError::Network("down".into()))
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Network(_)));
        assert_eq!(
            std::fs::read(dir.path().join("token_cache.enc")).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn corrupt_cache_is_quarantined_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        std::fs::write(dir.path().join("token_cache.enc"), b"garbage").unwrap();
        let t = cache
            .get_or_fetch("k", Freshness::Cached, || async {
                Ok(token("ya29.new", 3600))
            })
            .await
            .unwrap();
        assert_eq!(t.expose_secret(), "ya29.new");
        let quarantined: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("token_cache.enc.corrupt-"))
            .collect();
        assert_eq!(quarantined.len(), 1);
        assert_eq!(
            std::fs::read(dir.path().join(&quarantined[0])).unwrap(),
            b"garbage"
        );
    }

    #[tokio::test]
    async fn concurrent_writers_do_not_lose_updates() {
        let dir = tempfile::tempdir().unwrap();
        let (ks, _) = memory_keystore(dir.path());
        let ks = Arc::new(ks);
        let mut handles = Vec::new();
        for i in 0..8 {
            let cache = TokenCache::new(
                &dir.path().join("token_cache.enc"),
                &dir.path().join("token_cache.lock"),
                Arc::clone(&ks),
            );
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_fetch(&format!("k{i}"), Freshness::Cached, || async move {
                        Ok(token(&format!("t{i}"), 3600))
                    })
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let data = std::fs::read(dir.path().join("token_cache.enc")).unwrap();
        let pt = ks.decrypt(Purpose::TokenCache, &data, "t").unwrap();
        let file: CacheFile = serde_json::from_slice(&pt).unwrap();
        assert_eq!(file.entries.len(), 8);
    }
}
