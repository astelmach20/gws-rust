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

//! Writing response payloads to files or stdout.
//!
//! Files are written atomically: bytes go to a temporary file in the target
//! directory, which is renamed over the target only after the full body was
//! received (and matched `Content-Length`). A failed or interrupted download
//! never leaves a partial file behind.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use super::output::Emitter;
use crate::discovery::{JsonSchemaProperty, RestDescription, RestMethod};
use crate::error::GwsError;
use crate::output_file::AtomicFile;
use crate::transport::next_chunk;

/// Where `-o/--output` sends a response payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    /// Write atomically to this file and print a JSON summary.
    File(PathBuf),
    /// `-o -`: stream the raw payload to stdout (explicit opt-in to non-JSON stdout).
    Stdout,
}

/// Largest non-JSON text body that is inlined into the JSON output.
pub(crate) const MAX_INLINE_TEXT: usize = 16 * 1024 * 1024;

/// Whether a content type is text that can be inlined as a JSON string.
pub(crate) fn is_textual(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    let essence = ct.split(';').next().unwrap_or("").trim();
    essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence,
            "application/xml"
                | "application/javascript"
                | "application/x-www-form-urlencoded"
                | "application/vnd.google-apps.script+json"
        )
}

/// JSON document describing a non-JSON text body.
pub(crate) fn wrap_text(status: u16, content_type: &str, raw: &[u8]) -> Result<Value, GwsError> {
    let text = std::str::from_utf8(raw).map_err(|_| {
        GwsError::Validation(format!(
            "The response ({content_type}, {} bytes) is not UTF-8 text. Pass -o PATH to save it \
             or -o - to stream the raw bytes to stdout.",
            raw.len()
        ))
    })?;
    Ok(json!({
        "status": "success",
        "httpStatus": status,
        "contentType": content_type,
        "bytes": raw.len(),
        "body": text,
    }))
}

/// Handle a non-JSON response: stream it to `target`, or inline it as a
/// JSON string when it is text and no target was given. Binary bodies without
/// a target are an error, so stdout stays JSON.
pub(crate) async fn deliver_non_json(
    response: reqwest::Response,
    content_type: &str,
    target: Option<&OutputTarget>,
    idle: Option<Duration>,
    emitter: &Emitter,
) -> Result<Option<Value>, GwsError> {
    if let Some(t) = target {
        return stream_binary(response, content_type, t, idle, emitter).await;
    }
    let status = response.status().as_u16();
    if !is_textual(content_type) {
        let size = response
            .content_length()
            .map(|n| format!(", {n} bytes"))
            .unwrap_or_default();
        return Err(GwsError::Validation(format!(
            "The response is binary ({content_type}{size}). Pass -o PATH to save it, \
             or -o - to stream the raw bytes to stdout."
        )));
    }
    let raw = read_limited(response, idle, MAX_INLINE_TEXT).await?;
    wrap_text(status, content_type, &raw).map(Some)
}

async fn read_limited(
    response: reqwest::Response,
    idle: Option<Duration>,
    max: usize,
) -> Result<Vec<u8>, GwsError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(&mut stream, idle).await? {
        if body.len() + chunk.len() > max {
            return Err(GwsError::Validation(format!(
                "The text response is larger than {} MiB; pass -o PATH to save it",
                max / (1024 * 1024)
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Stream a response body into `path` atomically, verifying it against
/// `Content-Length` before the file is moved into place. Returns the number of
/// bytes written.
pub(crate) async fn save_stream(
    response: reqwest::Response,
    path: &Path,
    overwrite: bool,
    idle: Option<Duration>,
) -> Result<u64, GwsError> {
    let expected = response.content_length();
    let mut file = AtomicFile::create(path, overwrite)?;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = next_chunk(&mut stream, idle).await? {
        file.write(&chunk).await?;
        total += chunk.len() as u64;
    }
    // Verify before committing so a truncated body never replaces the target.
    check_length(expected, total)?;
    file.commit().await?;
    Ok(total)
}

/// Stream a binary response to `target`.
///
/// Returns the summary object printed for file downloads (`None` for stdout,
/// where printing a summary would corrupt the payload).
pub(crate) async fn stream_binary(
    response: reqwest::Response,
    content_type: &str,
    target: &OutputTarget,
    idle: Option<Duration>,
    emitter: &Emitter,
) -> Result<Option<Value>, GwsError> {
    match target {
        OutputTarget::Stdout => {
            let expected = response.content_length();
            let mut stream = response.bytes_stream();
            let mut total: u64 = 0;
            while let Some(chunk) = next_chunk(&mut stream, idle).await? {
                emitter.bytes(&chunk).await?;
                total += chunk.len() as u64;
            }
            emitter.flush().await?;
            check_length(expected, total)?;
            Ok(None)
        }
        OutputTarget::File(path) => {
            // `-o PATH` on a generated API method always replaces the target.
            let total = save_stream(response, path, true, idle).await?;
            Ok(Some(json!({
                "status": "success",
                "saved_file": path.display().to_string(),
                "mimeType": content_type,
                "bytes": total,
            })))
        }
    }
}

fn check_length(expected: Option<u64>, actual: u64) -> Result<(), GwsError> {
    match expected {
        Some(e) if e != actual => Err(GwsError::other(anyhow::anyhow!(
            "download truncated: Content-Length was {e} bytes but {actual} were received"
        ))),
        _ => Ok(()),
    }
}

/// Decode standard or URL-safe base64, padded or not (Gmail uses unpadded
/// base64url for attachment and message bodies).
pub(crate) fn decode_base64(text: &str) -> Result<Vec<u8>, GwsError> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    let config =
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
    let cleaned: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let engine = if cleaned.contains(['-', '_']) {
        GeneralPurpose::new(&base64::alphabet::URL_SAFE, config)
    } else {
        GeneralPurpose::new(&base64::alphabet::STANDARD, config)
    };
    engine
        .decode(cleaned.as_bytes())
        .map_err(|e| GwsError::Validation(format!("field is not valid base64: {e}")))
}

/// Resolve a dotted path (`payload.body.data`) in a JSON value.
fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |v, key| v.get(key))
}

/// Top-level response properties declared `format: "byte"` (base64 payloads).
fn byte_fields(doc: &RestDescription, method: &RestMethod) -> Vec<String> {
    let Some(schema) = method
        .response
        .as_ref()
        .and_then(|r| r.schema_ref.as_deref())
        .and_then(|name| doc.schemas.get(name))
    else {
        return Vec::new();
    };
    let is_bytes = |p: &JsonSchemaProperty| {
        p.prop_type.as_deref() == Some("string") && p.format.as_deref() == Some("byte")
    };
    let mut fields: Vec<String> = schema
        .properties
        .iter()
        .filter(|(_, p)| is_bytes(p))
        .map(|(k, _)| k.clone())
        .collect();
    fields.sort();
    fields
}

/// The base64 payload of a JSON response, decoded.
///
/// * `decode_field` given: that (dotted) field must hold base64 data.
/// * Otherwise, if the response schema has exactly one top-level
///   `format: byte` field and it is present (e.g. gmail `attachments.get`
///   `data`), it is decoded automatically.
/// * Otherwise `None`: the JSON document itself is the payload.
pub(crate) fn decoded_payload(
    doc: &RestDescription,
    method: &RestMethod,
    value: &Value,
    decode_field: Option<&str>,
) -> Result<Option<(String, Vec<u8>)>, GwsError> {
    let auto = byte_fields(doc, method);
    let field = match decode_field {
        Some(f) => f.to_string(),
        None => match auto.as_slice() {
            [only] if value.get(only).is_some_and(Value::is_string) => only.clone(),
            _ => return Ok(None),
        },
    };
    let text = lookup_path(value, &field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            GwsError::Validation(format!(
                "--decode-field '{field}' is not a string field of the response"
            ))
        })?;
    let bytes = decode_base64(text)
        .map_err(|e| GwsError::Validation(format!("--decode-field '{field}': {e}")))?;
    Ok(Some((field, bytes)))
}

/// Save a JSON response to `path`: the decoded payload when there is one
/// (see [`decoded_payload`]), otherwise the pretty-printed JSON document.
pub(crate) async fn save_json_response(
    doc: &RestDescription,
    method: &RestMethod,
    value: &Value,
    path: &Path,
    decode_field: Option<&str>,
) -> Result<Value, GwsError> {
    let (bytes, decoded_from) = match decoded_payload(doc, method, value, decode_field)? {
        Some((field, bytes)) => (bytes, Some(field)),
        None => {
            let mut text = serde_json::to_vec_pretty(value).map_err(|e| {
                GwsError::other(anyhow::anyhow!("failed to serialize response: {e}"))
            })?;
            text.push(b'\n');
            (text, None)
        }
    };
    crate::output_file::write_atomic(path, &bytes, true).await?;
    let mut summary = json!({
        "status": "success",
        "saved_file": path.display().to_string(),
        "bytes": bytes.len(),
    });
    if let Some(f) = decoded_from {
        summary["decodedField"] = json!(f);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{JsonSchema, SchemaRef};
    use std::collections::HashMap;

    #[test]
    fn textual_detection_and_wrapping() {
        assert!(is_textual("text/csv; charset=utf-8"));
        assert!(is_textual("application/vnd.google-apps.script+json"));
        assert!(is_textual("application/xml"));
        assert!(!is_textual("application/pdf"));
        assert!(!is_textual("image/png"));
        let v = wrap_text(200, "text/plain", b"a\x1b[31mb").unwrap();
        // Control characters stay inside a JSON string, escaped on output.
        assert!(serde_json::to_string(&v).unwrap().contains("\\u001b"));
        assert_eq!(v["bytes"], 7);
        assert!(wrap_text(200, "text/plain", &[0xff, 0xfe]).is_err());
    }

    #[test]
    fn base64_variants() {
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        // URL-safe alphabet: 0xfb 0xff -> "-_8"
        assert_eq!(decode_base64("-_8").unwrap(), vec![0xfb, 0xff]);
        assert_eq!(decode_base64("+/8=").unwrap(), vec![0xfb, 0xff]);
        assert!(decode_base64("!!!").is_err());
    }

    #[test]
    fn length_check() {
        assert!(check_length(None, 5).is_ok());
        assert!(check_length(Some(5), 5).is_ok());
        assert!(check_length(Some(6), 5).is_err());
    }

    fn attachment_doc() -> (RestDescription, RestMethod) {
        let mut props = HashMap::new();
        props.insert(
            "data".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".into()),
                format: Some("byte".into()),
                ..Default::default()
            },
        );
        props.insert(
            "size".to_string(),
            JsonSchemaProperty {
                prop_type: Some("integer".into()),
                ..Default::default()
            },
        );
        let mut schemas = HashMap::new();
        schemas.insert(
            "MessagePartBody".to_string(),
            JsonSchema {
                properties: props,
                ..Default::default()
            },
        );
        let doc = RestDescription {
            schemas,
            ..Default::default()
        };
        let method = RestMethod {
            response: Some(SchemaRef {
                schema_ref: Some("MessagePartBody".into()),
                parameter_name: None,
            }),
            ..Default::default()
        };
        (doc, method)
    }

    #[tokio::test]
    async fn json_response_auto_decodes_byte_field() {
        let (doc, method) = attachment_doc();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("att.bin");
        let summary = save_json_response(
            &doc,
            &method,
            &json!({"data": "aGVsbG8", "size": 5}),
            &path,
            None,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(summary["decodedField"], "data");
    }

    #[tokio::test]
    async fn json_response_explicit_field_and_plain_json() {
        let doc = RestDescription::default();
        let method = RestMethod::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("raw.eml");
        save_json_response(
            &doc,
            &method,
            &json!({"raw": "aGVsbG8"}),
            &path,
            Some("raw"),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");

        let err = save_json_response(&doc, &method, &json!({"x": 1}), &path, Some("raw"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a string field"));

        let json_path = dir.path().join("resp.json");
        save_json_response(&doc, &method, &json!({"x": 1}), &json_path, None)
            .await
            .unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(written, json!({"x": 1}));
    }

    #[tokio::test]
    async fn atomic_write_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        std::fs::write(&path, b"old").unwrap();
        crate::output_file::write_atomic(&path, b"new", true)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}
