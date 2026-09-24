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

//! Attachments: reading `--attach` files, fetching original parts, and downloading.

use super::prelude::*;

/// Fetch binary data for selected original parts, converting them to `Attachment`s.
///
/// Performs a size preflight check using metadata before downloading, then fetches
/// parts sequentially. `existing_bytes` is the cumulative size of user-supplied
/// `--attach` files, counted against the combined size limit.
pub(super) async fn fetch_original_parts(
    api: &GmailApi,
    message_id: &str,
    parts: &[OriginalPart],
    existing_bytes: u64,
) -> Result<Vec<Attachment>, GwsError> {
    // Size preflight: check metadata sizes before downloading anything
    let total_metadata_size: u64 = parts.iter().map(|p| p.size).sum();
    if existing_bytes.saturating_add(total_metadata_size) > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(GwsError::Validation(format!(
            "Original attachments ({:.1} MB) plus user attachments ({:.1} MB) exceed {}MB limit",
            total_metadata_size as f64 / (1024.0 * 1024.0),
            existing_bytes as f64 / (1024.0 * 1024.0),
            MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024),
        )));
    }

    tracing::info!(
        "Fetching {} original attachment(s) ({:.1} MB)...",
        parts.len(),
        total_metadata_size as f64 / (1024.0 * 1024.0),
    );

    let mut attachments = Vec::with_capacity(parts.len());
    let mut actual_bytes = existing_bytes;

    for part in parts {
        let data = match &part.data {
            PartData::AttachmentId(id) => api.get_attachment(message_id, id).await?,
            PartData::Inline(b64) => decode_base64url(b64).map_err(|e| {
                GwsError::other(format!(
                    "Invalid inline data for part '{}': {e}",
                    part.filename
                ))
            })?,
        };

        actual_bytes += data.len() as u64;
        if actual_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(GwsError::Validation(format!(
                "Total attachment size exceeds {}MB limit (after downloading '{}')",
                MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024),
                part.filename,
            )));
        }

        attachments.push(Attachment {
            filename: part.filename.clone(),
            content_type: part.content_type.clone(),
            data,
            content_id: part.content_id.clone(),
        });
    }

    Ok(attachments)
}

/// Fetch selected original parts and merge them into an existing attachment list.
///
/// Shared by `+forward` and `+reply`/`+reply-all` handlers. The caller is
/// responsible for filtering `parts` to the desired subset before calling
/// this function.
pub(super) async fn fetch_and_merge_original_parts(
    api: &GmailApi,
    message_id: &str,
    parts: &[OriginalPart],
    attachments: &mut Vec<Attachment>,
) -> Result<(), GwsError> {
    if parts.is_empty() {
        return Ok(());
    }
    let user_bytes: u64 = attachments.iter().map(|a| a.data.len() as u64).sum();
    let fetched = fetch_original_parts(api, message_id, parts, user_bytes).await?;
    attachments.extend(fetched);
    Ok(())
}

/// Gmail API upload endpoint limit is 35MB (per discovery document). Messages are
/// sent as multipart/related with the raw RFC 5322 message as the media part, so
/// the limit applies to the entire MIME message including headers, body, and
/// base64-encoded attachments. 25MB raw attachments ≈ 33MB with base64 + overhead.
pub(super) const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

/// A file attachment ready to add to an outgoing message.
///
/// Created either from a local file (`--attach`, where `content_type` is
/// inferred from the extension via `mime_guess2`) or from an original
/// message's MIME part (`fetch_original_parts`, where `content_type` comes
/// from the Gmail API). mail-builder handles RFC 2231 encoding for non-ASCII
/// filenames in the Content-Disposition header.
#[derive(Debug)]
pub(super) struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
    /// When present, this part is an inline image. Used by `finalize_message` to
    /// place the part inside a `multipart/related` container with `.inline().cid()`.
    pub content_id: Option<String>,
}

impl Attachment {
    /// Whether this attachment is an inline image (has a Content-ID) vs a regular file.
    pub fn is_inline(&self) -> bool {
        self.content_id.is_some()
    }
}

/// Read and validate attachments from `--attach` arguments.
///
/// Rejects control characters in paths, non-regular files, empty files,
/// and total size exceeding `MAX_TOTAL_ATTACHMENT_BYTES`.
///
/// Absolute and relative paths are both allowed. Unlike `--output-dir` (where
/// write confinement matters), `--attach` only reads files the user's process
/// already has access to. Path traversal restrictions would not prevent data
/// exfiltration — an agent could read any file via other means (e.g., shell
/// commands). The real mitigation for agent misuse is `--dry-run` and human
/// review of the command before execution.
pub(super) fn parse_attachments(matches: &ArgMatches) -> Result<Vec<Attachment>, GwsError> {
    let paths: Vec<&String> = crate::args::values(matches, "attach")?
        .map(|v| v.collect())
        .unwrap_or_default();

    let mut attachments = Vec::with_capacity(paths.len());
    let mut total_bytes: u64 = 0;

    for path in paths {
        let canonical = crate::validate::validate_safe_file_path(path, "--attach")?;

        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| GwsError::Validation(format!("Cannot read --attach '{path}': {e}")))?;
        if !metadata.is_file() {
            return Err(GwsError::Validation(format!(
                "--attach '{path}' is not a regular file"
            )));
        }

        let data = std::fs::read(&canonical)
            .map_err(|e| GwsError::Validation(format!("Cannot read --attach '{path}': {e}")))?;
        if data.is_empty() {
            return Err(GwsError::Validation(format!(
                "--attach '{path}' is empty (0 bytes)"
            )));
        }
        // Size check uses actual bytes read, not metadata, to avoid TOCTOU race
        total_bytes += data.len() as u64;
        if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(GwsError::Validation(format!(
                "Total attachment size exceeds {}MB limit",
                MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
            )));
        }
        // file_name() is None for paths like "/", "..", or "." — already caught by is_file().
        // to_str() is None only for non-UTF-8 filenames — impossible since path is &String.
        let filename = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                GwsError::Validation(format!("--attach '{path}': could not extract filename"))
            })?;
        let content_type = mime_guess2::from_path(&canonical)
            .first_or_octet_stream()
            .to_string();

        attachments.push(Attachment {
            filename: filename.to_string(),
            content_type,
            data,
            content_id: None,
        });
    }

    Ok(attachments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_attach_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test").arg(
            Arg::new("attach")
                .short('a')
                .long("attach")
                .action(ArgAction::Append),
        );
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_attachments_rejects_control_chars() {
        let matches = make_attach_matches(&["test", "-a", "file\0name.pdf"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(err.to_string().contains("control characters"));
    }

    #[test]
    fn test_parse_attachments_rejects_directory() {
        // Use a relative directory that exists in CWD
        let matches = make_attach_matches(&["test", "-a", "src"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }

    #[test]
    fn test_parse_attachments_empty_returns_empty_vec() {
        let matches = make_attach_matches(&["test"]);
        let attachments = parse_attachments(&matches).unwrap();
        assert!(attachments.is_empty());
    }

    #[test]
    fn test_parse_attachments_reads_real_file() {
        use std::io::Write;
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let dir = tempfile::tempdir_in(&cwd).unwrap();
        let file_path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let attachments = parse_attachments(&matches).unwrap();

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "test.txt");
        assert_eq!(attachments[0].content_type, "text/plain");
        assert_eq!(attachments[0].data, b"hello world");
    }

    #[test]
    fn test_parse_attachments_nonexistent_file() {
        let matches = make_attach_matches(&["test", "-a", "nonexistent_file.pdf"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("nonexistent_file.pdf"),
            "error should include the path: {}",
            err
        );
    }

    #[test]
    fn test_parse_attachments_unknown_extension_falls_back_to_octet_stream() {
        use std::io::Write;
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let dir = tempfile::tempdir_in(&cwd).unwrap();
        let file_path = dir.path().join("data.zzqqxx");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"unknown format").unwrap();
        drop(f);

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let attachments = parse_attachments(&matches).unwrap();

        assert_eq!(attachments[0].content_type, "application/octet-stream");
    }

    #[test]
    fn test_parse_attachments_size_limit_accumulates() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let dir = tempfile::tempdir_in(&cwd).unwrap();

        // Create two files whose combined size exceeds MAX_TOTAL_ATTACHMENT_BYTES
        let file1 = dir.path().join("big1.bin");
        let file2 = dir.path().join("big2.bin");
        // Each file is just over half the limit
        let half_plus_one = (MAX_TOTAL_ATTACHMENT_BYTES / 2 + 1) as usize;
        std::fs::write(&file1, vec![0u8; half_plus_one]).unwrap();
        std::fs::write(&file2, vec![0u8; half_plus_one]).unwrap();

        let path1 = file1.to_str().unwrap().to_string();
        let path2 = file2.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path1, "-a", &path2]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("exceeds"),
            "error should mention exceeding limit: {}",
            err
        );

        // A single file under the limit should succeed
        let matches = make_attach_matches(&["test", "-a", &path1]);
        assert!(parse_attachments(&matches).is_ok());
    }

    #[test]
    fn test_parse_attachments_rejects_empty_file() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let dir = tempfile::tempdir_in(&cwd).unwrap();
        let file_path = dir.path().join("empty.txt");
        std::fs::write(&file_path, b"").unwrap();

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("empty (0 bytes)"),
            "error should mention empty file: {}",
            err
        );
    }
}
