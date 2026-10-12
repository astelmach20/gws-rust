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

//! Atomic writing of user-requested output files (`-o/--output`, exported
//! documents, pulled script sources) — the one implementation shared by the
//! generic API executor and the `+helper` commands.
//!
//! Bytes go to a temporary file in the target directory, which is fsynced and
//! renamed over the target only when the caller commits, so a failed or
//! interrupted write never leaves a partial file behind. Without `overwrite`
//! an existing target is refused both up front and at the rename
//! (`persist_noclobber`), so a file created in between is not clobbered.
//!
//! Private configuration and credential files use
//! [`crate::fs_util::atomic_write`] instead; see there for why.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::error::GwsError;

fn exists_error(path: &Path) -> GwsError {
    GwsError::Validation(format!(
        "'{}' already exists; pass --overwrite to replace it",
        path.display()
    ))
}

/// A file being written atomically; nothing is visible at the target until
/// [`AtomicFile::commit`]. Dropping it without committing deletes the
/// temporary file.
pub(crate) struct AtomicFile {
    file: tokio::fs::File,
    temp: tempfile::TempPath,
    target: PathBuf,
    overwrite: bool,
}

impl AtomicFile {
    /// Start writing `target`, creating its parent directories. Fails with a
    /// validation error when the target is a directory, or exists and
    /// `overwrite` is false.
    pub(crate) fn create(target: &Path, overwrite: bool) -> Result<Self, GwsError> {
        // A directory can never be replaced by the rename in `commit`; refuse
        // it before any data is written (and whatever `overwrite` says).
        crate::validate::reject_directory(target, "output path")?;
        if !overwrite && target.exists() {
            return Err(exists_error(target));
        }
        let dir = match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        std::fs::create_dir_all(&dir).map_err(|e| {
            GwsError::other(anyhow::anyhow!(
                "failed to create directory '{}': {e}",
                dir.display()
            ))
        })?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(".gwsr-output-");
        // Output files are ordinary user files: create them with the default
        // mode (0666 minus the umask), not tempfile's private 0600.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o666));
        }
        let named = builder.tempfile_in(&dir).map_err(|e| {
            GwsError::other(anyhow::anyhow!(
                "failed to create a temporary file in '{}': {e}",
                dir.display()
            ))
        })?;
        let (file, temp) = named.into_parts();
        Ok(Self {
            file: tokio::fs::File::from_std(file),
            temp,
            target: target.to_path_buf(),
            overwrite,
        })
    }

    /// Append bytes.
    pub(crate) async fn write(&mut self, data: &[u8]) -> Result<(), GwsError> {
        self.file.write_all(data).await.map_err(|e| {
            GwsError::other(anyhow::anyhow!(
                "failed to write '{}': {e}",
                self.target.display()
            ))
        })
    }

    /// Flush, fsync and move the file into place.
    pub(crate) async fn commit(mut self) -> Result<(), GwsError> {
        let target = self.target.display().to_string();
        self.file
            .flush()
            .await
            .map_err(|e| GwsError::other(anyhow::anyhow!("failed to flush '{target}': {e}")))?;
        self.file
            .sync_all()
            .await
            .map_err(|e| GwsError::other(anyhow::anyhow!("failed to sync '{target}': {e}")))?;
        drop(self.file);
        let persisted = if self.overwrite {
            self.temp.persist(&self.target)
        } else {
            self.temp.persist_noclobber(&self.target)
        };
        persisted.map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                exists_error(&self.target)
            } else {
                GwsError::other(anyhow::anyhow!(
                    "failed to move output into '{target}': {}",
                    e.error
                ))
            }
        })
    }
}

/// Write `data` to `path` atomically, replacing an existing file only when
/// `overwrite` is set.
pub(crate) async fn write_atomic(
    path: &Path,
    data: &[u8],
    overwrite: bool,
) -> Result<(), GwsError> {
    let mut f = AtomicFile::create(path, overwrite)?;
    f.write(data).await?;
    f.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_atomic_respects_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("f.txt");
        write_atomic(&p, b"a", false).await.unwrap();
        let err = write_atomic(&p, b"b", false).await.unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert_eq!(std::fs::read(&p).unwrap(), b"a");
        write_atomic(&p, b"c", true).await.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"c");
    }

    #[tokio::test]
    async fn a_directory_target_is_refused_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out");
        std::fs::create_dir(&target).unwrap();
        for overwrite in [false, true] {
            let err = write_atomic(&target, b"x", overwrite).await.unwrap_err();
            assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
            assert!(err.to_string().contains("is a directory"), "{err}");
        }
        assert!(target.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn commit_does_not_clobber_a_file_created_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        let mut f = AtomicFile::create(&p, false).unwrap();
        f.write(b"new").await.unwrap();
        std::fs::write(&p, b"racer").unwrap();
        let err = f.commit().await.unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert_eq!(std::fs::read(&p).unwrap(), b"racer");
        // The temporary file was cleaned up.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn uncommitted_file_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        let mut f = AtomicFile::create(&p, true).unwrap();
        f.write(b"partial").await.unwrap();
        drop(f);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_files_get_the_default_mode_not_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        // A plain `fs::write` gets 0666 minus the umask; ours must match it.
        let plain = dir.path().join("plain.txt");
        std::fs::write(&plain, b"x").unwrap();
        let p = dir.path().join("f.txt");
        write_atomic(&p, b"x", false).await.unwrap();
        assert_eq!(mode(&p), mode(&plain));
    }
}
