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
use tokio::io::AsyncWriteExt;

use super::errors::other;
use super::output::Emitter;
use super::transport::next_chunk;
use crate::discovery::{JsonSchemaProperty, RestDescription, RestMethod};
use crate::error::GwsError;

/// Where binary output goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BinaryTarget {
    File(PathBuf),
    Stdout,
}

/// Decide where a binary response goes: `-o` when given; stdout when it is
/// not a terminal; otherwise refuse (binary on a terminal is never useful).
pub(crate) fn binary_target(
    output: Option<&Path>,
    stdout_is_terminal: bool,
    content_type: &str,
) -> Result<BinaryTarget, GwsError> {
    match output {
        Some(p) => Ok(BinaryTarget::File(p.to_path_buf())),
        None if !stdout_is_terminal => Ok(BinaryTarget::Stdout),
        None => Err(GwsError::Validation(format!(
            "The response is binary ({content_type}). Pass -o/--output <PATH> to save it, \
             or redirect stdout to a file or pipe."
        ))),
    }
}

struct AtomicFile {
    file: tokio::fs::File,
    temp: tempfile::TempPath,
    target: PathBuf,
}

impl AtomicFile {
    fn create(target: &Path) -> Result<Self, GwsError> {
        let dir = match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let named = tempfile::Builder::new()
            .prefix(".gwsr-download-")
            .tempfile_in(&dir)
            .map_err(|e| {
                other(anyhow::anyhow!(
                    "failed to create a temporary file in '{}': {e}",
                    dir.display()
                ))
            })?;
        let (file, temp) = named.into_parts();
        Ok(Self {
            file: tokio::fs::File::from_std(file),
            temp,
            target: target.to_path_buf(),
        })
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), GwsError> {
        self.file.write_all(data).await.map_err(|e| {
            other(anyhow::anyhow!(
                "failed to write '{}': {e}",
                self.target.display()
            ))
        })
    }

    async fn commit(mut self) -> Result<(), GwsError> {
        let target = self.target.display().to_string();
        self.file
            .flush()
            .await
            .map_err(|e| other(anyhow::anyhow!("failed to flush '{target}': {e}")))?;
        self.file
            .sync_all()
            .await
            .map_err(|e| other(anyhow::anyhow!("failed to sync '{target}': {e}")))?;
        drop(self.file);
        self.temp.persist(&self.target).map_err(|e| {
            other(anyhow::anyhow!(
                "failed to move download into '{target}': {e}"
            ))
        })
    }
}

/// Write `data` to `path` atomically.
pub(crate) async fn write_file_atomic(path: &Path, data: &[u8]) -> Result<(), GwsError> {
    let mut f = AtomicFile::create(path)?;
    f.write(data).await?;
    f.commit().await
}

/// Stream a binary response to `target`.
///
/// Returns the summary object printed for file downloads (`None` for stdout,
/// where printing a summary would corrupt the payload).
pub(crate) async fn stream_binary(
    response: reqwest::Response,
    content_type: &str,
    target: &BinaryTarget,
    idle: Option<Duration>,
    emitter: &Emitter,
) -> Result<Option<Value>, GwsError> {
    let expected = response.content_length();
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;

    match target {
        BinaryTarget::Stdout => {
            while let Some(chunk) = next_chunk(&mut stream, idle).await? {
                emitter.bytes(&chunk).await?;
                total += chunk.len() as u64;
            }
            emitter.flush().await?;
            check_length(expected, total)?;
            Ok(None)
        }
        BinaryTarget::File(path) => {
            let mut file = AtomicFile::create(path)?;
            while let Some(chunk) = next_chunk(&mut stream, idle).await? {
                file.write(&chunk).await?;
                total += chunk.len() as u64;
            }
            // Verify before committing so a truncated body never replaces the target.
            check_length(expected, total)?;
            file.commit().await?;
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
        Some(e) if e != actual => Err(other(anyhow::anyhow!(
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

/// Save a JSON response to `path`.
///
/// * `decode_field` given: that (dotted) field must hold base64 data, which is
///   decoded and written as raw bytes.
/// * Otherwise, if the response schema has exactly one top-level
///   `format: byte` field and it is present (e.g. gmail `attachments.get`
///   `data`), it is decoded automatically.
/// * Otherwise the JSON document itself is written.
pub(crate) async fn save_json_response(
    doc: &RestDescription,
    method: &RestMethod,
    value: &Value,
    path: &Path,
    decode_field: Option<&str>,
) -> Result<Value, GwsError> {
    let auto = byte_fields(doc, method);
    let field = match decode_field {
        Some(f) => Some(f.to_string()),
        None => match auto.as_slice() {
            [only] if value.get(only).is_some_and(Value::is_string) => Some(only.clone()),
            _ => None,
        },
    };

    let (bytes, decoded_from) = match &field {
        Some(f) => {
            let text = lookup_path(value, f)
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GwsError::Validation(format!(
                        "--decode-field '{f}' is not a string field of the response"
                    ))
                })?;
            let bytes = decode_base64(text)
                .map_err(|e| GwsError::Validation(format!("--decode-field '{f}': {e}")))?;
            (bytes, Some(f.clone()))
        }
        None => {
            let mut text = serde_json::to_vec_pretty(value)
                .map_err(|e| other(anyhow::anyhow!("failed to serialize response: {e}")))?;
            text.push(b'\n');
            (text, None)
        }
    };

    write_file_atomic(path, &bytes).await?;
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
    fn target_selection() {
        let p = Path::new("out.bin");
        assert_eq!(
            binary_target(Some(p), true, "x").unwrap(),
            BinaryTarget::File(p.to_path_buf())
        );
        assert_eq!(
            binary_target(None, false, "x").unwrap(),
            BinaryTarget::Stdout
        );
        let err = binary_target(None, true, "application/pdf").unwrap_err();
        assert!(err.to_string().contains("-o/--output"));
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
        write_file_atomic(&path, b"new").await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}
