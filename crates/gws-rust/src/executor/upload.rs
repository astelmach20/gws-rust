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

//! Media uploads: multipart (small files) and resumable (large files).

use std::io::IsTerminal;

use futures_util::StreamExt;
use futures_util::stream::TryStreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, LOCATION, RANGE};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use gws_rust_core::client::{Idempotency, RetryPolicy, Sent};

use crate::error::GwsError;
use crate::transport::{Transport, upload_timeout};

/// Files larger than this are uploaded with the resumable protocol when the
/// method supports it (Google recommends resumable above 5 MiB).
pub(crate) const RESUMABLE_THRESHOLD: u64 = 5 * 1024 * 1024;
/// Default resumable chunk size. Must be a multiple of 256 KiB.
pub(crate) const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;
/// Chunk sizes must be multiples of this.
pub(crate) const CHUNK_GRANULARITY: usize = 256 * 1024;
/// Consecutive failed chunk attempts tolerated before giving up.
const MAX_CHUNK_FAILURES: u32 = 5;

/// How to upload media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UploadMode {
    /// Resumable above [`RESUMABLE_THRESHOLD`] when supported, else multipart.
    #[default]
    Auto,
    /// Always resumable (`--upload-resumable`).
    Resumable,
}

/// An upload file on disk, re-read for every attempt and chunk.
pub(crate) struct UploadData {
    pub(crate) path: String,
    pub(crate) size: u64,
}

impl UploadData {
    pub(crate) fn len(&self) -> u64 {
        self.size
    }

    async fn read_range(&self, start: u64, len: usize) -> Result<bytes::Bytes, GwsError> {
        let path = &self.path;
        {
            {
                let mut file = tokio::fs::File::open(path).await.map_err(|e| {
                    GwsError::Validation(format!("failed to open upload file '{path}': {e}"))
                })?;
                file.seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(|e| {
                        GwsError::other(anyhow::anyhow!("failed to seek in '{path}': {e}"))
                    })?;
                let mut buf = vec![0u8; len];
                file.read_exact(&mut buf).await.map_err(|e| {
                    GwsError::other(anyhow::anyhow!(
                        "failed to read bytes {start}..{} of '{path}' (did the file change during upload?): {e}",
                        start + len as u64
                    ))
                })?;
                Ok(buf.into())
            }
        }
    }
}

/// Resolves the MIME type for the uploaded media content.
///
/// Priority: explicit `--upload-content-type`, then file extension, then the
/// metadata `mimeType`, then `application/octet-stream`. Extension ranks above
/// metadata because in Drive's model metadata `mimeType` is the *target* type
/// (e.g. a Google Doc) while the media `Content-Type` describes the bytes.
/// Control characters are stripped to prevent MIME header injection.
pub(crate) fn resolve_upload_mime(
    explicit: Option<&str>,
    upload_path: Option<&str>,
    metadata: &Option<Value>,
) -> String {
    let raw = explicit
        .map(str::to_string)
        .or_else(|| {
            upload_path.and_then(|path| mime_guess2::from_path(path).first().map(|m| m.to_string()))
        })
        .or_else(|| {
            metadata
                .as_ref()
                .and_then(|m| m.get("mimeType"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let sanitized: String = raw.chars().filter(|c| !c.is_control()).collect();
    if sanitized.is_empty() {
        "application/octet-stream".to_string()
    } else {
        sanitized
    }
}

fn metadata_json(metadata: &Option<Value>) -> Result<String, GwsError> {
    match metadata {
        Some(m) => serde_json::to_string(m)
            .map_err(|e| GwsError::Validation(format!("Failed to serialize upload metadata: {e}"))),
        None => Ok("{}".to_string()),
    }
}

/// Framing of a multipart/related upload body, computed once and reused
/// for every attempt (a streamed body can be consumed only once, so the body
/// itself is rebuilt per attempt).
pub(crate) struct MultipartFrame {
    preamble: String,
    postamble: String,
    pub(crate) content_type: String,
}

impl MultipartFrame {
    pub(crate) fn new(metadata: &Option<Value>, media_mime: &str) -> Result<Self, GwsError> {
        let boundary = format!("gwsr_boundary_{:016x}", rand::random::<u64>());
        let metadata_json = metadata_json(metadata)?;
        Ok(Self {
            preamble: format!(
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata_json}\r\n\
                 --{boundary}\r\nContent-Type: {media_mime}\r\n\r\n"
            ),
            postamble: format!("\r\n--{boundary}--\r\n"),
            content_type: format!("multipart/related; boundary={boundary}"),
        })
    }

    pub(crate) fn content_length(&self, data: &UploadData) -> u64 {
        self.preamble.len() as u64 + data.len() + self.postamble.len() as u64
    }

    pub(crate) fn body(&self, data: &UploadData) -> reqwest::Body {
        {
            {
                let path = &data.path;
                // Stream the file so memory stays O(64 KiB) regardless of size.
                let path = path.clone();
                let file_stream = futures_util::stream::once(async move {
                    tokio::fs::File::open(&path).await.map_err(|e| {
                        std::io::Error::new(
                            e.kind(),
                            format!("failed to open upload file '{path}': {e}"),
                        )
                    })
                })
                .map_ok(tokio_util::io::ReaderStream::new)
                .try_flatten();
                let pre = bytes::Bytes::from(self.preamble.clone().into_bytes());
                let post = bytes::Bytes::from(self.postamble.clone().into_bytes());
                let stream = futures_util::stream::once(async { Ok::<_, std::io::Error>(pre) })
                    .chain(file_stream)
                    .chain(futures_util::stream::once(async {
                        Ok::<_, std::io::Error>(post)
                    }));
                reqwest::Body::wrap_stream(stream)
            }
        }
    }
}

/// Send a multipart upload. `query` must already contain `uploadType=multipart`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_multipart(
    transport: &Transport,
    method: Method,
    url: &str,
    query: &[(String, String)],
    metadata: &Option<Value>,
    data: &UploadData,
    media_mime: &str,
    idempotency: Idempotency,
) -> Result<Sent, GwsError> {
    let frame = MultipartFrame::new(metadata, media_mime)?;
    let policy = RetryPolicy {
        response_timeout: upload_timeout(transport.retry.response_timeout, data.len()),
        ..transport.retry.clone()
    };
    transport
        .send_with_policy(&policy, method, url, idempotency, |rb| {
            rb.query(query)
                .header(CONTENT_TYPE, frame.content_type.as_str())
                .header(CONTENT_LENGTH, frame.content_length(data))
                .body(frame.body(data))
        })
        .await
}

/// Parse a resumable-upload `Range: bytes=0-N` header into the next offset.
pub(crate) fn next_offset(headers: &reqwest::header::HeaderMap) -> Result<u64, GwsError> {
    let Some(value) = headers.get(RANGE) else {
        return Ok(0);
    };
    let text = value
        .to_str()
        .map_err(|_| GwsError::other(anyhow::anyhow!("non-ASCII Range header in upload status")))?;
    let last = text
        .strip_prefix("bytes=")
        .and_then(|r| r.split_once('-'))
        .and_then(|(_, end)| end.trim().parse::<u64>().ok())
        .ok_or_else(|| {
            GwsError::other(anyhow::anyhow!("unparseable upload Range header {text:?}"))
        })?;
    Ok(last + 1)
}

struct Progress {
    enabled: bool,
    total: u64,
}

impl Progress {
    fn new(total: u64) -> Self {
        Self {
            enabled: std::io::stderr().is_terminal(),
            total,
        }
    }

    fn update(&self, done: u64) {
        if !self.enabled || self.total == 0 {
            return;
        }
        let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
        crate::output::eprint_text(&format!(
            "\rUploading: {:.1} / {:.1} MiB ({:.0}%)",
            mib(done),
            mib(self.total),
            done as f64 * 100.0 / self.total as f64
        ));
    }

    fn finish(&self) {
        if self.enabled && self.total > 0 {
            crate::output::eprint_line("");
        }
    }
}

/// Parameters for [`resumable_upload`].
pub(crate) struct Resumable<'a> {
    pub method: Method,
    pub url: &'a str,
    pub query: &'a [(String, String)],
    pub metadata: &'a Option<Value>,
    pub data: &'a UploadData,
    pub media_mime: &'a str,
    pub chunk_size: usize,
}

/// Upload with Google's resumable protocol.
///
/// 1. Start a session (`uploadType=resumable`); the `Location` header is the
///    session URI.
/// 2. `PUT` chunks with `Content-Range`; `308` means "keep going" and its
///    `Range` header says how much the server has persisted.
/// 3. On a network error, timeout or 5xx, ask the server how much it has
///    (`Content-Range: bytes */total`) and resume from there, up to
///    [`MAX_CHUNK_FAILURES`] consecutive failures.
///
/// Returns the final response (200/201 with the created resource), or the
/// first non-retryable error response for the caller to map.
pub(crate) async fn resumable_upload(
    transport: &Transport,
    req: Resumable<'_>,
) -> Result<Sent, GwsError> {
    if req.chunk_size == 0 || !req.chunk_size.is_multiple_of(CHUNK_GRANULARITY) {
        return Err(GwsError::Validation(format!(
            "resumable chunk size must be a positive multiple of {CHUNK_GRANULARITY} bytes"
        )));
    }
    let total = req.data.len();
    let metadata = metadata_json(req.metadata)?;
    let has_metadata = req.metadata.is_some();

    // Starting a session creates nothing, so it is safe to retry.
    let init = transport
        .send(req.method.clone(), req.url, Idempotency::Idempotent, |rb| {
            let rb = rb
                .query(req.query)
                .header("X-Upload-Content-Type", req.media_mime)
                .header("X-Upload-Content-Length", total);
            if has_metadata {
                rb.header(CONTENT_TYPE, "application/json; charset=UTF-8")
                    .body(metadata.clone())
            } else {
                rb.header(CONTENT_LENGTH, 0)
            }
        })
        .await?;
    if !init.response.status().is_success() {
        return Ok(init);
    }
    let session = init
        .response
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            GwsError::other(anyhow::anyhow!(
                "resumable upload session started (HTTP {}) but no Location header was returned",
                init.response.status()
            ))
        })?;

    // Chunks are retried by the resume logic below, not by the transport:
    // after a failure the server may have persisted part of the chunk.
    let chunk_policy = RetryPolicy {
        max_attempts: 1,
        response_timeout: upload_timeout(transport.retry.response_timeout, req.chunk_size as u64),
        ..transport.retry.clone()
    };
    let status_policy = RetryPolicy {
        max_attempts: 1,
        ..transport.retry.clone()
    };

    let progress = Progress::new(total);
    let mut offset: u64 = 0;
    let mut failures: u32 = 0;
    loop {
        let len = usize::try_from((total - offset).min(req.chunk_size as u64))
            .map_err(|_| GwsError::other(anyhow::anyhow!("chunk length overflow")))?;
        let chunk = req.data.read_range(offset, len).await?;
        let range = if total == 0 {
            "bytes */0".to_string()
        } else {
            format!("bytes {offset}-{}/{total}", offset + len as u64 - 1)
        };
        let result = transport
            .send_with_policy(
                &chunk_policy,
                Method::PUT,
                &session,
                Idempotency::Idempotent,
                |rb| {
                    rb.header(CONTENT_RANGE, range.as_str())
                        .header(CONTENT_LENGTH, chunk.len())
                        .body(chunk.clone())
                },
            )
            .await;

        let failed_reason = match result {
            Ok(sent) => match sent.response.status() {
                s if s.is_success() => {
                    progress.update(total);
                    progress.finish();
                    return Ok(sent);
                }
                StatusCode::PERMANENT_REDIRECT => {
                    offset = next_offset(sent.response.headers())?;
                    failures = 0;
                    progress.update(offset);
                    continue;
                }
                StatusCode::NOT_FOUND | StatusCode::GONE => {
                    progress.finish();
                    return Err(GwsError::other(anyhow::anyhow!(
                        "resumable upload session expired (HTTP {}); rerun the command to start a new upload",
                        sent.response.status()
                    )));
                }
                s if gws_rust_core::client::is_retryable_status(s) => format!("HTTP {s}"),
                _ => {
                    progress.finish();
                    return Ok(sent);
                }
            },
            Err(e) => e.to_string(),
        };

        failures += 1;
        if failures > MAX_CHUNK_FAILURES {
            progress.finish();
            return Err(GwsError::other(anyhow::anyhow!(
                "resumable upload failed after {MAX_CHUNK_FAILURES} consecutive retries at byte {offset} of {total}: {failed_reason}"
            )));
        }
        tracing::debug!(offset, failures, reason = %failed_reason, "upload chunk failed; querying session status");
        tokio::time::sleep(transport.retry.backoff(failures - 1)).await;

        // Ask the server how much it has.
        let status_range = format!("bytes */{total}");
        match transport
            .send_with_policy(
                &status_policy,
                Method::PUT,
                &session,
                Idempotency::Idempotent,
                |rb| {
                    rb.header(CONTENT_RANGE, status_range.as_str())
                        .header(CONTENT_LENGTH, 0)
                },
            )
            .await
        {
            Ok(sent) if sent.response.status().is_success() => {
                progress.update(total);
                progress.finish();
                return Ok(sent);
            }
            Ok(sent) if sent.response.status() == StatusCode::PERMANENT_REDIRECT => {
                offset = next_offset(sent.response.headers())?;
            }
            Ok(sent) => {
                tracing::debug!(status = %sent.response.status(), "upload status query failed; retrying chunk");
            }
            Err(e) => tracing::debug!(error = %e, "upload status query failed; retrying chunk"),
        }
    }
}

/// Time budget helper exposed for tests.
#[cfg(test)]
pub(crate) fn chunk_timeout(
    base: Option<std::time::Duration>,
    chunk: usize,
) -> Option<std::time::Duration> {
    upload_timeout(base, chunk as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn resolve_mime_priority() {
        let md = Some(json!({ "mimeType": "application/vnd.google-apps.document" }));
        assert_eq!(
            resolve_upload_mime(Some("text/markdown"), Some("f.txt"), &md),
            "text/markdown"
        );
        assert_eq!(
            resolve_upload_mime(None, Some("notes.md"), &md),
            "text/markdown"
        );
        let md_plain = Some(json!({ "mimeType": "text/plain" }));
        assert_eq!(
            resolve_upload_mime(None, Some("file.unknown"), &md_plain),
            "text/plain"
        );
        assert_eq!(
            resolve_upload_mime(None, Some("data.csv"), &None),
            "text/csv"
        );
        assert_eq!(
            resolve_upload_mime(None, Some("file.unknown"), &None),
            "application/octet-stream"
        );
        assert_eq!(
            resolve_upload_mime(Some("text/plain\r\nX: y"), None, &None),
            "text/plainX: y"
        );
    }

    #[test]
    fn multipart_framing() {
        let data = UploadData {
            path: "unused".to_string(),
            size: 11,
        };
        let md = Some(json!({"name": "test.txt"}));
        let frame = MultipartFrame::new(&md, "text/plain").unwrap();
        let (content_type, len) = (frame.content_type.clone(), frame.content_length(&data));
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let pre = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{{\"name\":\"test.txt\"}}\r\n\
             --{boundary}\r\nContent-Type: text/plain\r\n\r\n"
        );
        let post = format!("\r\n--{boundary}--\r\n");
        assert_eq!(len, (pre.len() + 11 + post.len()) as u64);
    }

    #[tokio::test]
    async fn multipart_file_content_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        std::fs::write(&path, vec![0xABu8; 256 * 1024]).unwrap();
        let data = UploadData {
            path: path.to_str().unwrap().to_string(),
            size: 256 * 1024,
        };
        let frame = MultipartFrame::new(&None, "application/octet-stream").unwrap();
        let (content_type, len) = (frame.content_type.clone(), frame.content_length(&data));
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let pre = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{{}}\r\n\
             --{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n"
        );
        let post = format!("\r\n--{boundary}--\r\n");
        assert_eq!(len, pre.len() as u64 + 256 * 1024 + post.len() as u64);
    }

    #[tokio::test]
    async fn read_range_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"0123456789").unwrap();
        let file = UploadData {
            path: path.to_str().unwrap().to_string(),
            size: 10,
        };
        assert_eq!(&file.read_range(3, 4).await.unwrap()[..], b"3456");
        assert!(
            file.read_range(8, 4).await.is_err(),
            "short read must fail loudly"
        );
    }

    #[test]
    fn range_header_parsing() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(next_offset(&h).unwrap(), 0);
        h.insert(RANGE, "bytes=0-262143".parse().unwrap());
        assert_eq!(next_offset(&h).unwrap(), 262_144);
        h.insert(RANGE, "garbage".parse().unwrap());
        assert!(next_offset(&h).is_err());
    }

    #[test]
    fn chunk_timeout_scales() {
        assert_eq!(
            chunk_timeout(Some(Duration::from_secs(60)), DEFAULT_CHUNK_SIZE),
            Some(Duration::from_secs(60 + 128))
        );
        assert_eq!(chunk_timeout(None, DEFAULT_CHUNK_SIZE), None);
    }
}
