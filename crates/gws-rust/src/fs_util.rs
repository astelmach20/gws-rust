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

//! File-system utilities for secret-bearing files.
//!
//! Every helper here fails loudly: permission problems, partial writes and
//! lock timeouts are reported as errors, never logged and ignored.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Write `data` to `path` atomically with 0600 permissions.
///
/// Uses `tempfile::NamedTempFile` (random name, `O_EXCL`, 0600 from creation),
/// fsyncs the file, renames it over `path`, then fsyncs the parent directory
/// so the rename itself is durable.
///
/// # Errors
///
/// Returns an `io::Error` if the temporary file cannot be created or written,
/// if the rename fails, or if the parent directory cannot be synced.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = parent_dir(path)?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| io::Error::new(e.error.kind(), e.error))?;
    sync_dir(parent)
}

fn parent_dir(path: &Path) -> io::Result<&Path> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => Ok(p),
        Some(_) => Ok(Path::new(".")),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path '{}' has no parent directory", path.display()),
        )),
    }
}

/// Fsync a directory so that renames/creations inside it are durable.
///
/// On Windows directories cannot be opened for syncing; NTFS metadata
/// journaling makes the rename durable, so this is a no-op there.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _unused = dir;
        Ok(())
    }
}

/// Create `dir` (and parents) and restrict it to the owner (0700 on Unix).
///
/// # Errors
///
/// Fails if the directory cannot be created or its permissions cannot be set.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Refuse secret files that are readable or writable by group/other.
///
/// This follows symlinks (the permissions of the target matter), mirroring
/// OpenSSH's handling of private keys. On non-Unix platforms this is a no-op.
///
/// # Errors
///
/// Returns an error naming the file, its mode and the `chmod` fix when the
/// file is accessible to anyone but its owner.
pub fn check_private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use anyhow::Context;
        use std::os::unix::fs::PermissionsExt;
        let meta =
            std::fs::metadata(path).with_context(|| format!("cannot stat '{}'", path.display()))?;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            anyhow::bail!(
                "refusing to use '{}': it contains secrets but is accessible by other users \
                 (mode {mode:04o}). Restrict it with: chmod 600 '{}'",
                path.display(),
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _unused = path;
    }
    Ok(())
}

/// An exclusive advisory lock on a lock file, released on drop.
///
/// Uses the standard library's `File::lock` family (flock on Unix,
/// `LockFileEx` on Windows), so it coordinates concurrent `gwsr` processes.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    /// Acquire an exclusive lock on `path`, creating it (0600) if needed.
    ///
    /// Polls with `try_lock` so a stuck peer produces a clear error after
    /// `timeout` instead of hanging forever.
    ///
    /// # Errors
    ///
    /// Fails if the lock file cannot be opened or the lock is not acquired
    /// within `timeout`.
    pub fn acquire(path: &Path, timeout: Duration) -> io::Result<Self> {
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(path)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        file,
                        path: path.to_path_buf(),
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "timed out after {}s waiting for lock '{}' (another gwsr process \
                                 may be stuck; if not, the lock is released automatically when \
                                 that process exits)",
                                timeout.as_secs(),
                                path.display()
                            ),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e),
            }
        }
    }

    /// Async variant of [`FileLock::acquire`] that waits on the blocking pool.
    pub async fn acquire_async(path: &Path, timeout: Duration) -> io::Result<Self> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || Self::acquire(&path, timeout))
            .await
            .map_err(io::Error::other)?
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing the descriptor releases the lock too; unlocking explicitly
        // just makes the release immediate. An unlock failure cannot leave the
        // lock held past process exit, so it is logged rather than propagated.
        if let Err(e) = self.file.unlock() {
            tracing::debug!(path = %self.path.display(), error = %e, "explicit unlock failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn atomic_write_creates_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        atomic_write(&path, b"data").unwrap();
        let files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|res| res.unwrap().file_name())
            .collect();
        assert_eq!(files, vec![std::ffi::OsString::from("credentials.enc")]);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_sets_0700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a").join("b");
        ensure_private_dir(&sub).unwrap();
        let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn check_private_file_rejects_group_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.json");
        fs::write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let err = check_private_file(&path).unwrap_err().to_string();
        assert!(err.contains("chmod 600"), "{err}");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        check_private_file(&path).unwrap();
    }

    #[test]
    fn file_lock_is_exclusive_and_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.lock");
        let held = FileLock::acquire(&path, Duration::from_secs(1)).unwrap();
        let err = FileLock::acquire(&path, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        drop(held);
        FileLock::acquire(&path, Duration::from_secs(1)).unwrap();
    }
}
