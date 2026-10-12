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

//! On-disk Discovery Document cache.
//!
//! Documents live in `<root>/discovery/<api>+<version>.json`. The directory
//! is created `0700` and files are written atomically (temp file + rename)
//! with mode `0600`, and only after the document has been parsed and its
//! endpoints validated (SEC-12). Files or directories writable by group or
//! others are never trusted.
//!
//! The cache root is always supplied by the caller; this module reads no
//! environment variables.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;

use super::error::DiscoveryError;
use super::model::RestDescription;
use super::parse_and_validate;

/// Default time a cached document is considered fresh.
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

const DISCOVERY_SUBDIR: &str = "discovery";
const TEMP_PREFIX: &str = ".tmp-";
/// How many times [`DiscoveryCache::clear`] relists a directory that other
/// processes keep writing to before giving up with an error.
const CLEAR_MAX_ROUNDS: usize = 5;

/// A cache of Discovery Documents rooted at a caller-chosen directory.
#[derive(Debug, Clone)]
pub struct DiscoveryCache {
    dir: PathBuf,
    ttl: Duration,
}

/// A document read from the cache.
#[derive(Debug)]
pub struct CachedDocument {
    pub doc: RestDescription,
    /// Time since the file was written. Zero if the mtime is in the future
    /// (such entries are never fresh).
    pub age: Duration,
    /// Whether `age` is within the cache TTL.
    pub is_fresh: bool,
}

/// The files [`DiscoveryCache::clear`] removes, for a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearPlan {
    /// The cache directory, removed last; `None` when it does not exist.
    pub dir: Option<PathBuf>,
    /// Cached discovery documents.
    pub documents: Vec<PathBuf>,
    /// Temporary files left by an interrupted write.
    pub temp_files: Vec<PathBuf>,
}

impl DiscoveryCache {
    /// A cache whose documents live under `root/discovery`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            dir: root.as_ref().join(DISCOVERY_SUBDIR),
            ttl: DEFAULT_TTL,
        }
    }

    /// Override the freshness TTL (default [`DEFAULT_TTL`]).
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The directory holding cached documents.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The TTL after which cached documents are refetched.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Path of the cache file for an (already validated) API name and version.
    /// `+` cannot appear in API identifiers, so names never collide.
    pub fn entry_path(&self, service: &str, version: &str) -> PathBuf {
        self.dir.join(format!("{service}+{version}.json"))
    }

    /// Read, parse, and validate a cached document.
    ///
    /// Returns `Ok(None)` when nothing is cached. A corrupt or untrusted file
    /// is an error ([`DiscoveryError::InvalidDocument`] or
    /// [`DiscoveryError::InsecurePermissions`]); it is not deleted here.
    pub(crate) fn load(
        &self,
        service: &str,
        version: &str,
        api_base_override: Option<&Url>,
    ) -> Result<Option<CachedDocument>, DiscoveryError> {
        match std::fs::symlink_metadata(&self.dir) {
            Ok(_) => check_not_shared(&self.dir)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(DiscoveryError::cache("inspect", &self.dir, e)),
        }
        let path = self.entry_path(service, version);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(DiscoveryError::cache("inspect", &path, e)),
        };
        if !meta.file_type().is_file() {
            return Err(DiscoveryError::InvalidDocument {
                origin: path.display().to_string(),
                reason: "cache entry is not a regular file".to_string(),
            });
        }
        check_not_shared(&path)?;
        let modified = meta
            .modified()
            .map_err(|e| DiscoveryError::cache("read mtime of", &path, e))?;
        // An mtime in the future is suspicious: never treat it as fresh.
        let (age, is_fresh) = match modified.elapsed() {
            Ok(age) => (age, age < self.ttl),
            Err(_) => (Duration::ZERO, false),
        };
        let body = std::fs::read(&path).map_err(|e| DiscoveryError::cache("read", &path, e))?;
        let doc = parse_and_validate(
            &body,
            service,
            version,
            api_base_override,
            &path.display().to_string(),
        )?;
        Ok(Some(CachedDocument { doc, age, is_fresh }))
    }

    /// Delete one cached entry. Missing entries are not an error.
    pub(crate) fn remove(&self, service: &str, version: &str) -> Result<(), DiscoveryError> {
        let path = self.entry_path(service, version);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(DiscoveryError::cache("remove", &path, e)),
        }
    }

    /// Atomically write an already-validated document body.
    pub(crate) fn store(
        &self,
        service: &str,
        version: &str,
        body: &[u8],
    ) -> Result<(), DiscoveryError> {
        self.ensure_dir()?;
        let path = self.entry_path(service, version);
        let mut tmp = tempfile::Builder::new()
            .prefix(TEMP_PREFIX)
            .suffix(".json")
            .tempfile_in(&self.dir)
            .map_err(|e| DiscoveryError::cache("create temp file in", &self.dir, e))?;
        set_mode(tmp.path(), 0o600)?;
        tmp.write_all(body)
            .and_then(|()| tmp.as_file().sync_all())
            .map_err(|e| DiscoveryError::cache("write", tmp.path().to_path_buf(), e))?;
        tmp.persist(&path)
            .map_err(|e| DiscoveryError::cache("persist", &path, e.error))?;
        Ok(())
    }

    /// What [`DiscoveryCache::clear`] would remove, without removing anything.
    ///
    /// # Errors
    ///
    /// The cache directory cannot be listed, or holds a subdirectory (which
    /// `clear` refuses to delete).
    pub fn clear_plan(&self) -> Result<ClearPlan, DiscoveryError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ClearPlan {
                    dir: None,
                    documents: Vec::new(),
                    temp_files: Vec::new(),
                });
            }
            Err(e) => return Err(DiscoveryError::cache("list", &self.dir, e)),
        };
        let mut documents = Vec::new();
        let mut temp_files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| DiscoveryError::cache("list", &self.dir, e))?;
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                // Removed (or renamed into place) since the listing.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(DiscoveryError::cache("inspect", &path, e)),
            };
            if file_type.is_dir() {
                return Err(DiscoveryError::cache(
                    "clear",
                    &path,
                    std::io::Error::other("unexpected directory inside the discovery cache"),
                ));
            }
            if entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                temp_files.push(path);
            } else {
                documents.push(path);
            }
        }
        documents.sort();
        temp_files.sort();
        Ok(ClearPlan {
            dir: Some(self.dir.clone()),
            documents,
            temp_files,
        })
    }

    /// Remove every cached document (and leftover temporary files), then the
    /// cache directory. Returns the number of documents removed.
    ///
    /// Only the cache's own `discovery` directory is touched; the root passed
    /// to [`DiscoveryCache::new`] is left in place.
    ///
    /// Other `gwsr` processes may use the cache at the same time: a file that
    /// disappears before it is deleted (a writer persisted its temp file, or
    /// another clear removed it) is skipped, and documents written while the
    /// clear runs are removed too. Any other failure is an error.
    pub fn clear(&self) -> Result<usize, DiscoveryError> {
        self.clear_from(self.clear_plan()?)
    }

    fn clear_from(&self, mut plan: ClearPlan) -> Result<usize, DiscoveryError> {
        let mut removed = 0;
        let mut rounds = 0;
        loop {
            let Some(dir) = &plan.dir else {
                return Ok(removed);
            };
            for path in plan.documents.iter().chain(&plan.temp_files) {
                match std::fs::remove_file(path) {
                    Ok(()) if !plan.temp_files.contains(path) => removed += 1,
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(DiscoveryError::cache("remove", path, e)),
                }
            }
            match std::fs::remove_dir(dir) {
                Ok(()) => return Ok(removed),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(removed),
                // A concurrent writer added files after the listing: list again.
                Err(e)
                    if e.kind() == std::io::ErrorKind::DirectoryNotEmpty
                        && rounds < CLEAR_MAX_ROUNDS =>
                {
                    rounds += 1;
                    plan = self.clear_plan()?;
                }
                Err(e) => return Err(DiscoveryError::cache("remove", dir, e)),
            }
        }
    }

    fn ensure_dir(&self) -> Result<(), DiscoveryError> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&self.dir)
            .map_err(|e| DiscoveryError::cache("create", &self.dir, e))?;
        // The directory may pre-date us with looser permissions; we own it.
        set_mode(&self.dir, 0o700)
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| DiscoveryError::cache("set permissions on", path, e))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), DiscoveryError> {
    Ok(())
}

/// Fail if `path` is writable by group or others.
#[cfg(unix)]
fn check_not_shared(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map_err(|e| DiscoveryError::cache("inspect", path, e))?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o022 != 0 {
        return Err(DiscoveryError::InsecurePermissions {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_not_shared(_path: &Path) -> Result<(), DiscoveryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const DOC: &str = r#"{"name":"drive","version":"v3","rootUrl":"https://www.googleapis.com/","servicePath":"drive/v3/"}"#;

    #[test]
    fn missing_dir_and_entry_are_none() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        assert!(cache.load("drive", "v3", None).unwrap().is_none());
        cache.store("gmail", "v1", br#"{}"#).unwrap();
        assert!(cache.load("drive", "v3", None).unwrap().is_none());
    }

    #[test]
    fn store_then_load_roundtrip_is_fresh() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let got = cache.load("drive", "v3", None).unwrap().unwrap();
        assert_eq!(got.doc.name, "drive");
        assert!(got.is_fresh);
        assert_eq!(
            cache.entry_path("drive", "v3"),
            tmp.path().join("discovery").join("drive+v3.json")
        );
    }

    #[test]
    fn zero_ttl_makes_entries_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path()).with_ttl(Duration::ZERO);
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        assert!(!cache.load("drive", "v3", None).unwrap().unwrap().is_fresh);
    }

    #[cfg(unix)]
    #[test]
    fn store_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let dir_mode = std::fs::metadata(cache.dir()).unwrap().permissions().mode() & 0o777;
        let file_mode = std::fs::metadata(cache.entry_path("drive", "v3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        // No temp files left behind.
        let names: Vec<_> = std::fs::read_dir(cache.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn store_tightens_preexisting_loose_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        std::fs::create_dir_all(cache.dir()).unwrap();
        std::fs::set_permissions(cache.dir(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            cache.load("drive", "v3", None),
            Err(DiscoveryError::InsecurePermissions { .. })
        ));
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        assert!(cache.load("drive", "v3", None).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_file_is_untrusted() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let path = cache.entry_path("drive", "v3");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(matches!(
            cache.load("drive", "v3", None),
            Err(DiscoveryError::InsecurePermissions { .. })
        ));
    }

    #[test]
    fn corrupt_or_untrusted_entries_are_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache
            .store("drive", "v3", b"<html>captive portal</html>")
            .unwrap();
        assert!(matches!(
            cache.load("drive", "v3", None),
            Err(DiscoveryError::InvalidDocument { .. })
        ));
        let evil = DOC.replace("https://www.googleapis.com/", "https://evil.example/");
        cache.store("drive", "v3", evil.as_bytes()).unwrap();
        let err = cache.load("drive", "v3", None).unwrap_err().to_string();
        assert!(err.contains("untrusted API base URL"), "{err}");
        // Name/version mismatch.
        cache.store("gmail", "v1", DOC.as_bytes()).unwrap();
        assert!(cache.load("gmail", "v1", None).is_err());
    }

    #[test]
    fn remove_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.remove("drive", "v3").unwrap();
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        cache.remove("drive", "v3").unwrap();
        assert!(cache.load("drive", "v3", None).unwrap().is_none());
    }

    #[test]
    fn clear_removes_only_discovery_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("unrelated.txt"), "keep me").unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        assert_eq!(cache.clear().unwrap(), 0);
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        cache.store("gmail", "v1", DOC.as_bytes()).unwrap();
        std::fs::write(cache.dir().join(".tmp-leftover.json"), "x").unwrap();
        assert_eq!(cache.clear().unwrap(), 2);
        assert!(!cache.dir().exists());
        assert!(tmp.path().join("unrelated.txt").exists());
    }

    #[test]
    fn clear_plan_lists_without_removing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        assert_eq!(cache.clear_plan().unwrap().dir, None);
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let leftover = cache.dir().join(format!("{TEMP_PREFIX}x.json"));
        std::fs::write(&leftover, "x").unwrap();
        let plan = cache.clear_plan().unwrap();
        assert_eq!(plan.dir.as_deref(), Some(cache.dir()));
        assert_eq!(plan.documents.len(), 1);
        assert_eq!(plan.temp_files, vec![leftover.clone()]);
        assert!(plan.documents[0].exists());
        assert!(leftover.exists());
    }

    #[test]
    fn clear_tolerates_files_removed_by_a_concurrent_writer() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        cache.store("gmail", "v1", DOC.as_bytes()).unwrap();
        let temp = cache.dir().join(format!("{TEMP_PREFIX}x.json"));
        std::fs::write(&temp, "x").unwrap();
        let plan = cache.clear_plan().unwrap();
        // Between listing and deleting, a writer persists (renames away) its
        // temp file and another `cache clear` deletes a document.
        std::fs::remove_file(&temp).unwrap();
        std::fs::remove_file(cache.entry_path("gmail", "v1")).unwrap();
        assert_eq!(cache.clear_from(plan).unwrap(), 1);
        assert!(!cache.dir().exists());
    }

    #[test]
    fn clear_removes_documents_added_during_the_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let plan = cache.clear_plan().unwrap();
        // A concurrent command caches another document mid-clear.
        cache.store("gmail", "v1", DOC.as_bytes()).unwrap();
        assert_eq!(cache.clear_from(plan).unwrap(), 2);
        assert!(!cache.dir().exists());
    }

    #[test]
    fn clear_tolerates_the_directory_vanishing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        let plan = cache.clear_plan().unwrap();
        // A concurrent `cache clear` finished first.
        std::fs::remove_dir_all(cache.dir()).unwrap();
        assert_eq!(cache.clear_from(plan).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn clear_still_reports_real_removal_errors() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        cache.store("drive", "v3", DOC.as_bytes()).unwrap();
        // Without write permission on the directory, unlinking fails with
        // PermissionDenied, which must not be swallowed.
        std::fs::set_permissions(cache.dir(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = cache.clear();
        std::fs::set_permissions(cache.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let err = result.unwrap_err();
        assert!(err.to_string().contains("remove"), "{err}");
        assert!(cache.entry_path("drive", "v3").exists());
    }

    #[test]
    fn clear_refuses_unexpected_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiscoveryCache::new(tmp.path());
        std::fs::create_dir_all(cache.dir().join("nested")).unwrap();
        assert!(cache.clear().is_err());
        assert!(cache.dir().join("nested").exists());
    }
}
