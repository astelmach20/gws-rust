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

//! `gmail +attachments`: download a message's attachments as decoded files.
//!
//! Filenames come from the (untrusted) sender, so they are reduced to a single
//! safe path component, de-duplicated, and never allowed to overwrite an
//! existing file unless `--overwrite` is given. Files are written atomically.

use super::cli::required_str;
use super::prelude::*;
use crate::helpers::modelarmor::SanitizeConfig;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Maximum filename length in bytes (common filesystem limit is 255).
const MAX_FILENAME_BYTES: usize = 200;

/// Reduce a sender-supplied filename to a safe single path component.
pub(super) fn safe_filename(raw: &str, fallback: &str) -> String {
    // Keep only the last component of any path the sender may have embedded.
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    let name = if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    };
    truncate_filename(&name)
}

/// Truncate to MAX_FILENAME_BYTES on a char boundary, preserving the extension.
fn truncate_filename(name: &str) -> String {
    if name.len() <= MAX_FILENAME_BYTES {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if name.len() - i <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let budget = MAX_FILENAME_BYTES.saturating_sub(ext.len());
    let mut end = budget.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ext}", &stem[..end])
}

/// Make `name` unique within `taken` by appending " (n)" before the extension.
fn dedupe(name: &str, taken: &mut HashSet<String>) -> String {
    if taken.insert(name.to_lowercase()) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 1;
    loop {
        let candidate = format!("{stem} ({n}){ext}");
        if taken.insert(candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// Choose output paths for the parts. Fails if any target exists and
/// `overwrite` is false, before anything is downloaded.
fn plan_paths(
    dir: &Path,
    parts: &[OriginalPart],
    overwrite: bool,
) -> Result<Vec<PathBuf>, GwsError> {
    let mut taken = HashSet::new();
    let mut paths = Vec::with_capacity(parts.len());
    let mut conflicts = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let name = dedupe(
            &safe_filename(&part.filename, &format!("attachment-{i}")),
            &mut taken,
        );
        let path = dir.join(&name);
        if !overwrite && path.exists() {
            conflicts.push(path.display().to_string());
        }
        paths.push(path);
    }
    if !conflicts.is_empty() {
        return Err(GwsError::Validation(format!(
            "Refusing to overwrite existing file(s): {} (use --overwrite)",
            conflicts.join(", ")
        )));
    }
    Ok(paths)
}

/// Write `data` to `path` atomically via a temp file in the same directory.
fn write_atomically(path: &Path, data: &[u8], overwrite: bool) -> Result<(), GwsError> {
    let dir = path
        .parent()
        .ok_or_else(|| GwsError::other(format!("{} has no parent directory", path.display())))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| {
        GwsError::other(format!(
            "Failed to create temp file in {}: {e}",
            dir.display()
        ))
    })?;
    tmp.write_all(data)
        .and_then(|()| tmp.as_file().sync_all())
        .map_err(|e| GwsError::other(format!("Failed to write {}: {e}", path.display())))?;
    let persisted = if overwrite {
        tmp.persist(path).map(|_| ())
    } else {
        tmp.persist_noclobber(path).map(|_| ())
    };
    persisted
        .map_err(|e| GwsError::other(format!("Failed to save {}: {}", path.display(), e.error)))
}

/// Select which parts to download.
fn select_parts(parts: &[OriginalPart], include_inline: bool) -> Vec<OriginalPart> {
    parts
        .iter()
        .filter(|p| include_inline || !p.is_inline())
        .cloned()
        .collect()
}

/// Download `parts` into `dir` and describe what was written.
async fn download(
    api: &GmailApi,
    message_id: &str,
    parts: &[OriginalPart],
    dir: &Path,
    overwrite: bool,
) -> Result<Vec<Value>, GwsError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| GwsError::other(format!("Failed to create {}: {e}", dir.display())))?;
    let paths = plan_paths(dir, parts, overwrite)?;
    let mut saved = Vec::with_capacity(parts.len());
    for (part, path) in parts.iter().zip(paths) {
        let data = match &part.data {
            PartData::AttachmentId(id) => api.get_attachment(message_id, id).await?,
            PartData::Inline(b64) => decode_base64url(b64).map_err(|e| {
                GwsError::other(format!(
                    "Invalid inline data for part '{}': {e}",
                    part.filename
                ))
            })?,
        };
        write_atomically(&path, &data, overwrite)?;
        saved.push(json!({
            "filename": part.filename,
            "path": path.display().to_string(),
            "mimeType": part.content_type,
            "size": data.len(),
        }));
    }
    Ok(saved)
}

/// Handle `+attachments`.
pub(super) async fn handle_attachments(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    if sanitize_config.template.is_some() {
        return Err(GwsError::Validation(
            "--sanitize is not supported by +attachments: binary files cannot be scanned"
                .to_string(),
        ));
    }
    let message_id = required_str(matches, "message-id")?;
    let dir = crate::validate::validate_safe_output_dir(&required_str(matches, "output-dir")?)?;
    let include_inline = crate::args::flag(matches, "include-inline")?;
    let overwrite = crate::args::flag(matches, "overwrite")?;

    if crate::args::dry_run(matches)? {
        let url = format!(
            "{}/users/me/messages/{}",
            super::api::GMAIL_API_BASE,
            crate::validate::encode_path_segment(&message_id)
        );
        return crate::helpers::http::print_dry_run(
            matches,
            vec![crate::helpers::http::dry_run_request(
                "GET",
                &url,
                &[("format", "full".to_string())],
                None,
            )],
        );
    }

    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let original = api.get_original_message(&message_id).await?;
    let parts = select_parts(&original.parts, include_inline);
    let saved = download(&api, &message_id, &parts, &dir, overwrite).await?;
    if saved.is_empty() {
        tracing::info!(
            "Message {} has no attachments to download",
            sanitize_for_terminal(&message_id)
        );
    }
    let out = json!({ "messageId": message_id, "files": saved });
    let format = crate::helpers::http::output_format(matches)?;
    crate::output::emit(&crate::formatter::format_value(&out, &format)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::mock_api;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn part(name: &str, data: PartData, inline: bool) -> OriginalPart {
        OriginalPart {
            filename: name.to_string(),
            content_type: "application/octet-stream".to_string(),
            size: 0,
            data,
            content_id: inline.then(|| "cid1".to_string()),
        }
    }

    #[test]
    fn safe_filename_strips_paths_and_specials() {
        assert_eq!(safe_filename("../../etc/passwd", "f"), "passwd");
        assert_eq!(safe_filename("..\\..\\win.ini", "f"), "win.ini");
        assert_eq!(safe_filename("a<b>:c?.txt", "f"), "a_b__c_.txt");
        assert_eq!(safe_filename("evil\u{0}\n.pdf", "f"), "evil__.pdf");
        assert_eq!(safe_filename("..", "fallback.bin"), "fallback.bin");
        assert_eq!(safe_filename(".bashrc", "f"), "bashrc");
        assert_eq!(safe_filename("   ", "f.bin"), "f.bin");
    }

    #[test]
    fn long_names_keep_extension() {
        let long = format!("{}.pdf", "é".repeat(300));
        let out = safe_filename(&long, "f");
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.ends_with(".pdf"));
    }

    #[test]
    fn dedupe_adds_suffix() {
        let mut taken = HashSet::new();
        assert_eq!(dedupe("a.pdf", &mut taken), "a.pdf");
        assert_eq!(dedupe("A.pdf", &mut taken), "A (1).pdf");
        assert_eq!(dedupe("a.pdf", &mut taken), "a (2).pdf");
        assert_eq!(dedupe("noext", &mut taken), "noext");
        assert_eq!(dedupe("noext", &mut taken), "noext (1)");
    }

    #[test]
    fn plan_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old").unwrap();
        let parts = vec![part("a.txt", PartData::Inline(String::new()), false)];
        let err = plan_paths(dir.path(), &parts, false).unwrap_err();
        assert!(err.to_string().contains("--overwrite"));
        assert!(plan_paths(dir.path(), &parts, true).is_ok());
    }

    #[test]
    fn select_parts_excludes_inline_by_default() {
        let parts = vec![
            part("a", PartData::Inline(String::new()), false),
            part("img", PartData::Inline(String::new()), true),
        ];
        assert_eq!(select_parts(&parts, false).len(), 1);
        assert_eq!(select_parts(&parts, true).len(), 2);
    }

    #[tokio::test]
    async fn download_writes_decoded_files() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m1/attachments/A1"))
            // "hello?" as unpadded base64url
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": "aGVsbG8_"})))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let parts = vec![
            part("../report.txt", PartData::AttachmentId("A1".into()), false),
            part(
                "report.txt",
                PartData::Inline(URL_SAFE_NO_PAD_ENCODE.to_string()),
                false,
            ),
        ];
        let saved = download(&mock_api(&server), "m1", &parts, dir.path(), false)
            .await
            .unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(
            std::fs::read(dir.path().join("report.txt")).unwrap(),
            b"hello?"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report (1).txt")).unwrap(),
            b"hi"
        );
    }

    /// "hi" as unpadded base64url.
    const URL_SAFE_NO_PAD_ENCODE: &str = "aGk";

    #[tokio::test]
    async fn download_failure_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let parts = vec![part("a.bin", PartData::AttachmentId("A1".into()), false)];
        assert!(
            download(&mock_api(&server), "m1", &parts, dir.path(), false)
                .await
                .is_err()
        );
        assert!(!dir.path().join("a.bin").exists());
    }
}
