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

//! `gwsr batch`: send many calls in `multipart/mixed` batch requests.
//!
//! Input is NDJSON, one call per line:
//!
//! ```json
//! {"id": "a", "method": "files.get", "params": {"fileId": "123"}}
//! {"method": "permissions.create", "params": {"fileId": "123"}, "json": {"role": "reader", "type": "anyone"}}
//! ```
//!
//! `method` is the resource path plus method name (the service prefix is
//! optional). Output is NDJSON, one line per call, in input order:
//! `{"id": ..., "status": 200, "body": ...}`. Calls are grouped into batches
//! of at most 100 (Google's limit). The command fails (exit 1) when any call
//! failed, after printing every result.

use std::io::IsTerminal;

use reqwest::Method;
use serde_json::{Map, Value, json};
use tokio::io::AsyncReadExt;

use gws_rust_core::client::{EndpointPolicy, Idempotency};

use super::errors::{error_from_response, other};
use super::output::Emitter;
use super::transport::{Credentials, Transport, read_body};
use super::url::{UrlTarget, api_root, build_url};
use super::{ExecOptions, body_schema, input, safety};
use crate::discovery::{RestDescription, RestMethod, RestResource};
use crate::error::GwsError;

/// Google's maximum number of calls per batch request.
pub(crate) const MAX_BATCH: usize = 100;

/// One parsed input line.
#[derive(Debug)]
pub(crate) struct BatchCall<'a> {
    pub id: String,
    pub method: &'a RestMethod,
    pub http: Method,
    /// Path and query relative to the API host, e.g. `/drive/v3/files/1?fields=id`.
    pub path_and_query: String,
    pub body: Option<Value>,
}

/// `gwsr batch` arguments. `--dry-run` and `--api-version` are global
/// options and arrive through [`BatchContext`].
#[derive(Debug, Clone, clap::Args)]
#[command(
    after_help = "Input line format:\n  {\"id\": \"optional\", \"method\": \"files.get\", \"params\": {...}, \"json\": {...}}\n\
Output line format:\n  {\"id\": \"...\", \"status\": 200, \"body\": {...}}"
)]
pub struct BatchArgs {
    /// API to call, e.g. drive or gmail:v1
    #[arg(value_name = "SERVICE[:VERSION]")]
    pub service: String,
    /// NDJSON file with one call per line (default: stdin)
    #[arg(long, short = 'i', value_name = "FILE")]
    pub input: Option<String>,
    /// Confirm destructive calls in the batch without prompting
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Send parameters that are not in the Discovery document
    #[arg(long)]
    pub allow_unknown_params: bool,
    /// Send body fields that are not in the Discovery schema
    #[arg(long)]
    pub allow_unknown_fields: bool,
}

/// Global options that apply to `gwsr batch`.
#[derive(Debug, Clone, Default)]
pub struct BatchContext {
    /// `--dry-run`: print the batch parts without sending them.
    pub dry_run: bool,
    /// `--api-version`: overrides the `:VERSION` suffix.
    pub api_version: Option<String>,
}

/// Entry point for `gwsr batch ...`.
pub async fn handle_batch_command(args: &BatchArgs, ctx: &BatchContext) -> Result<(), GwsError> {
    let resolved =
        crate::services::resolve_service_spec(&args.service, ctx.api_version.as_deref())?;
    let doc =
        crate::discovery::fetch_discovery_document(&resolved.api_name, &resolved.version).await?;

    let text = match args.input.as_deref() {
        Some(path) => {
            let safe = crate::validate::validate_safe_file_path(path, "--input")?;
            tokio::fs::read_to_string(&safe).await.map_err(|e| {
                GwsError::Validation(format!("--input: failed to read '{path}': {e}"))
            })?
        }
        None => {
            if std::io::stdin().is_terminal() {
                return Err(GwsError::Validation(
                    "gwsr batch reads NDJSON calls from stdin or --input FILE".to_string(),
                ));
            }
            let mut buf = String::new();
            tokio::io::stdin()
                .read_to_string(&mut buf)
                .await
                .map_err(|e| GwsError::Validation(format!("failed to read stdin: {e}")))?;
            buf
        }
    };

    let mut options = ExecOptions::from_env()?;
    options.dry_run = ctx.dry_run;
    options.assume_yes = args.yes;
    options.allow_unknown_params = args.allow_unknown_params;
    options.allow_unknown_fields = args.allow_unknown_fields;

    let calls = parse_calls(&doc, &text, &options)?;
    let credentials = if options.dry_run {
        Credentials::None
    } else {
        batch_credentials(&calls).await?
    };
    run_batch(&doc, &calls, credentials, &options, &Emitter::stdout()).await
}

async fn batch_credentials(calls: &[BatchCall<'_>]) -> Result<Credentials, GwsError> {
    let mut scopes: Vec<String> = Vec::new();
    for call in calls {
        let chosen = crate::auth::scopes_for_method(&call.method.scopes, &call.method.http_method)
            .map_err(|e| GwsError::Auth(format!("{e:#}")))?;
        scopes.extend(chosen);
    }
    scopes.sort_unstable();
    scopes.dedup();
    super::options::credentials_for_scopes(&scopes).await
}

/// Resolve `files.get`, `drive.files.get` or `permissions.list` style names.
pub(crate) fn find_method<'a>(doc: &'a RestDescription, name: &str) -> Option<&'a RestMethod> {
    let prefix = format!("{}.", doc.name);
    let path: Vec<&str> = name
        .strip_prefix(&prefix)
        .unwrap_or(name)
        .split('.')
        .collect();
    let (method_name, resources) = path.split_last()?;
    let mut current: Option<&RestResource> = None;
    for r in resources {
        let map = match current {
            None => &doc.resources,
            Some(res) => &res.resources,
        };
        current = Some(map.get(*r)?);
    }
    current?.methods.get(*method_name)
}

/// Parse and validate NDJSON calls.
pub(crate) fn parse_calls<'a>(
    doc: &'a RestDescription,
    text: &str,
    options: &ExecOptions,
) -> Result<Vec<BatchCall<'a>>, GwsError> {
    let mut calls = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let at =
            |msg: String| GwsError::Validation(format!("batch input line {}: {msg}", lineno + 1));
        let entry: Value =
            serde_json::from_str(line).map_err(|e| at(format!("invalid JSON: {e}")))?;
        let obj = entry
            .as_object()
            .ok_or_else(|| at("expected a JSON object".to_string()))?;
        let name = obj
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| at("missing string field \"method\"".to_string()))?;
        let method = find_method(doc, name)
            .ok_or_else(|| at(format!("unknown method '{name}' in the {} API", doc.name)))?;
        if method.supports_media_upload && obj.contains_key("upload") {
            return Err(at(
                "media uploads are not supported in batch requests".to_string()
            ));
        }
        let params: Map<String, Value> = match obj.get("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Err(at("\"params\" must be an object".to_string())),
        };
        let body = obj.get("json").filter(|v| !v.is_null()).cloned();
        input::validate_params(doc, method, &params, options.allow_unknown_params)
            .map_err(|e| at(e.to_string()))?;
        if let (Some(b), Some(schema)) = (
            &body,
            method
                .request
                .as_ref()
                .and_then(|r| r.schema_ref.as_deref()),
        ) {
            body_schema::validate_body(b, schema, doc, options.allow_unknown_fields)
                .map_err(|e| at(e.to_string()))?;
        }
        let url = build_url(doc, method, &params, UrlTarget::Method, &options.endpoints)
            .map_err(|e| at(e.to_string()))?;
        let path_and_query = relative_target(doc, &options.endpoints, &url.url, &url.query)?;
        let http = super::http_method(method)?;
        let id = match obj.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            None | Some(Value::Null) => (calls.len() + 1).to_string(),
            Some(_) => return Err(at("\"id\" must be a string or number".to_string())),
        };
        if id.contains(['\r', '\n', '<', '>']) {
            return Err(at("\"id\" must not contain CR, LF, '<' or '>'".to_string()));
        }
        calls.push(BatchCall {
            id,
            method,
            http,
            path_and_query,
            body,
        });
    }
    if calls.is_empty() {
        return Err(GwsError::Validation(
            "batch input contains no calls".to_string(),
        ));
    }
    Ok(calls)
}

/// Path and query of `url` for the inner request line.
fn relative_target(
    doc: &RestDescription,
    endpoints: &EndpointPolicy,
    url: &str,
    query: &[(String, String)],
) -> Result<String, GwsError> {
    let mut parsed = reqwest::Url::parse(url)
        .map_err(|e| other(anyhow::anyhow!("invalid request URL {url}: {e}")))?;
    if !query.is_empty() {
        parsed.query_pairs_mut().extend_pairs(query);
    }
    let root = reqwest::Url::parse(&api_root(doc, endpoints))
        .map_err(|e| other(anyhow::anyhow!("invalid API root: {e}")))?;
    if parsed.host_str() != root.host_str() {
        return Err(GwsError::Validation(format!(
            "method URL host {:?} differs from the batch endpoint host {:?}",
            parsed.host_str(),
            root.host_str()
        )));
    }
    Ok(match parsed.query() {
        Some(q) => format!("{}?{q}", parsed.path()),
        None => parsed.path().to_string(),
    })
}

/// Build a multipart/mixed body. Returns `(content_type, body)`.
pub(crate) fn build_batch_body(
    calls: &[BatchCall<'_>],
    boundary: &str,
) -> Result<(String, String), GwsError> {
    let mut body = String::new();
    for call in calls {
        body.push_str(&format!(
            "--{boundary}\r\nContent-Type: application/http\r\nContent-ID: <item-{}>\r\n\r\n",
            call.id
        ));
        body.push_str(&format!(
            "{} {} HTTP/1.1\r\n",
            call.http, call.path_and_query
        ));
        match &call.body {
            Some(b) => {
                let json = serde_json::to_string(b)
                    .map_err(|e| other(anyhow::anyhow!("failed to serialize batch body: {e}")))?;
                body.push_str(&format!(
                    "Content-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\n\r\n{json}\r\n",
                    json.len()
                ));
            }
            None => body.push_str("\r\n"),
        }
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    Ok((format!("multipart/mixed; boundary={boundary}"), body))
}

/// One decoded part of a batch response.
#[derive(Debug, PartialEq)]
pub(crate) struct PartResponse {
    pub content_id: Option<String>,
    pub status: u16,
    pub body: Value,
}

fn boundary_of(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|p| {
        let p = p.trim();
        p.strip_prefix("boundary=")
            .map(|b| b.trim_matches('"').to_string())
    })
}

/// Split `text` into its header block and body at the first blank line.
fn split_headers(text: &str) -> (&str, &str) {
    for sep in ["\r\n\r\n", "\n\n"] {
        if let Some(i) = text.find(sep) {
            return (&text[..i], &text[i + sep.len()..]);
        }
    }
    (text, "")
}

/// Parse a multipart/mixed batch response.
pub(crate) fn parse_batch_response(
    content_type: &str,
    body: &str,
) -> Result<Vec<PartResponse>, GwsError> {
    let boundary = boundary_of(content_type).ok_or_else(|| {
        other(anyhow::anyhow!(
            "batch response has no multipart boundary (Content-Type: {content_type})"
        ))
    })?;
    let delimiter = format!("--{boundary}");
    let mut parts = Vec::new();
    for raw in body.split(delimiter.as_str()).skip(1) {
        if raw.starts_with("--") {
            break;
        }
        let raw = raw.trim_start_matches(['\r', '\n']);
        let (outer_headers, http) = split_headers(raw);
        let content_id = outer_headers.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("content-id").then(|| {
                v.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string()
            })
        });
        let (status_and_headers, inner_body) = split_headers(http);
        let status_line = status_and_headers.lines().next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| {
                other(anyhow::anyhow!(
                    "malformed batch part status line {status_line:?}"
                ))
            })?;
        let inner_body = inner_body.trim_end_matches(['\r', '\n']);
        let body = if inner_body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(inner_body)
                .unwrap_or_else(|_| Value::String(inner_body.to_string()))
        };
        parts.push(PartResponse {
            content_id,
            status,
            body,
        });
    }
    Ok(parts)
}

pub(crate) async fn run_batch(
    doc: &RestDescription,
    calls: &[BatchCall<'_>],
    credentials: Credentials,
    options: &ExecOptions,
    emitter: &Emitter,
) -> Result<(), GwsError> {
    let destructive: Vec<&str> = calls
        .iter()
        .filter(|c| safety::is_destructive(c.method))
        .map(|c| c.id.as_str())
        .collect();
    let batch_url = format!(
        "{}batch/{}/{}",
        api_root(doc, &options.endpoints),
        doc.name,
        doc.version
    );

    if options.dry_run {
        for chunk in calls.chunks(MAX_BATCH) {
            let (content_type, body) = build_batch_body(chunk, "batch_dry_run")?;
            emitter.value(
                &json!({"dry_run": true, "url": batch_url, "contentType": content_type, "body": body}),
                &crate::formatter::OutputFormat::Json,
            )?;
        }
        return Ok(());
    }
    if !destructive.is_empty() {
        safety::confirm(
            &format!(
                "This batch ({} destructive call(s): {})",
                destructive.len(),
                destructive.join(", ")
            ),
            options.assume_yes,
            options.confirm,
            options.interactive,
            safety::ask_on_terminal,
        )?;
    }

    let transport = Transport::new(
        &credentials,
        options.retry.clone(),
        options.endpoints.clone(),
        if options.quota_project {
            crate::auth::get_quota_project()
        } else {
            None
        },
    )?;
    let auth = credentials.auth_method();
    let mut failures = 0usize;
    for chunk in calls.chunks(MAX_BATCH) {
        let boundary = format!("gwsr_batch_{:016x}", rand::random::<u64>());
        let (content_type, body) = build_batch_body(chunk, &boundary)?;
        let all_idempotent = chunk
            .iter()
            .all(|c| matches!(c.http, Method::GET | Method::PUT | Method::DELETE));
        let idem = if all_idempotent {
            Idempotency::Idempotent
        } else {
            Idempotency::NonIdempotent
        };
        let sent = transport
            .send(Method::POST, &batch_url, idem, |rb| {
                rb.header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                    .body(body.clone())
            })
            .await?;
        let status = sent.response.status();
        let note = sent.retry_note();
        let resp_type = sent
            .response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let raw = read_body(sent.response, options.retry.response_timeout).await?;
        let text = String::from_utf8_lossy(&raw);
        if !status.is_success() {
            return Err(error_from_response(status, &text, &auth, note.as_deref()));
        }
        let parts = parse_batch_response(&resp_type, &text)?;
        if parts.len() != chunk.len() {
            return Err(other(anyhow::anyhow!(
                "batch response has {} parts for {} calls",
                parts.len(),
                chunk.len()
            )));
        }
        for (i, part) in parts.into_iter().enumerate() {
            // Responses carry `response-item-<id>`; fall back to position.
            let id = part
                .content_id
                .as_deref()
                .and_then(|c| c.strip_prefix("response-item-"))
                .map(str::to_string)
                .unwrap_or_else(|| chunk[i].id.clone());
            if !(200..300).contains(&part.status) {
                failures += 1;
            }
            emitter
                .line(&json!({"id": id, "status": part.status, "body": part.body}).to_string())?;
        }
    }
    if failures > 0 {
        return Err(GwsError::Api {
            code: 207,
            message: format!(
                "{failures} of {} batch call(s) failed; see the per-call results on stdout",
                calls.len()
            ),
            reason: "batchPartialFailure".to_string(),
            enable_url: None,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::MethodParameter;

    pub(crate) fn drive_doc(root: &str) -> RestDescription {
        let mut get_params = std::collections::HashMap::new();
        get_params.insert(
            "fileId".to_string(),
            MethodParameter {
                location: Some("path".into()),
                required: true,
                param_type: Some("string".into()),
                ..Default::default()
            },
        );
        let mut files = RestResource::default();
        files.methods.insert(
            "get".into(),
            RestMethod {
                id: Some("drive.files.get".into()),
                http_method: "GET".into(),
                path: "files/{fileId}".into(),
                parameters: get_params.clone(),
                ..Default::default()
            },
        );
        files.methods.insert(
            "delete".into(),
            RestMethod {
                id: Some("drive.files.delete".into()),
                http_method: "DELETE".into(),
                path: "files/{fileId}".into(),
                parameters: get_params,
                ..Default::default()
            },
        );
        let mut doc = RestDescription {
            name: "drive".into(),
            version: "v3".into(),
            root_url: root.into(),
            service_path: "drive/v3/".into(),
            ..Default::default()
        };
        doc.parameters
            .insert("fields".into(), MethodParameter::default());
        doc.resources.insert("files".into(), files);
        doc
    }

    fn opts() -> ExecOptions {
        let mut o = ExecOptions::from_env().unwrap();
        o.endpoints = EndpointPolicy::google_only();
        o
    }

    #[test]
    fn method_lookup() {
        let doc = drive_doc("https://www.googleapis.com/");
        assert!(find_method(&doc, "files.get").is_some());
        assert!(find_method(&doc, "drive.files.get").is_some());
        assert!(find_method(&doc, "files.nope").is_none());
        assert!(find_method(&doc, "get").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn parses_and_builds_body() {
        let doc = drive_doc("https://www.googleapis.com/");
        let calls = parse_calls(
            &doc,
            "{\"id\":\"a\",\"method\":\"files.get\",\"params\":{\"fileId\":\"1\",\"fields\":\"id\"}}\n\n{\"method\":\"files.delete\",\"params\":{\"fileId\":\"2\"}}\n",
            &opts(),
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].path_and_query, "/drive/v3/files/1?fields=id");
        assert_eq!(calls[1].id, "2");
        let (ct, body) = build_batch_body(&calls, "B").unwrap();
        assert_eq!(ct, "multipart/mixed; boundary=B");
        assert!(
            body.contains(
                "Content-ID: <item-a>\r\n\r\nGET /drive/v3/files/1?fields=id HTTP/1.1\r\n"
            )
        );
        assert!(body.contains("DELETE /drive/v3/files/2 HTTP/1.1"));
        assert!(body.ends_with("--B--\r\n"));
    }

    #[test]
    #[serial_test::serial]
    fn rejects_bad_lines() {
        let doc = drive_doc("https://www.googleapis.com/");
        for (line, needle) in [
            ("nope", "invalid JSON"),
            ("{\"method\":\"files.zzz\"}", "unknown method"),
            ("{\"method\":\"files.get\",\"params\":{}}", "fileId"),
            (
                "{\"method\":\"files.get\",\"params\":{\"fileId\":\"1\",\"fieldz\":1}}",
                "did you mean 'fields'",
            ),
        ] {
            let err = parse_calls(&doc, line, &opts()).unwrap_err().to_string();
            assert!(err.contains(needle), "{line}: {err}");
        }
        assert!(parse_calls(&doc, "\n", &opts()).is_err());
    }

    #[test]
    fn parses_multipart_response() {
        let body = "--resp\r\nContent-Type: application/http\r\nContent-ID: <response-item-a>\r\n\r\n\
                    HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"id\":\"1\"}\r\n\
                    --resp\r\nContent-Type: application/http\r\nContent-ID: <response-item-b>\r\n\r\n\
                    HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n\r\n{\"error\":{\"code\":404}}\r\n\
                    --resp\r\nContent-Type: application/http\r\n\r\nHTTP/1.1 204 No Content\r\n\r\n\r\n--resp--\r\n";
        let parts = parse_batch_response("multipart/mixed; boundary=resp", body).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].content_id.as_deref(), Some("response-item-a"));
        assert_eq!(parts[0].body, json!({"id": "1"}));
        assert_eq!(parts[1].status, 404);
        assert_eq!(parts[2].status, 204);
        assert_eq!(parts[2].body, Value::Null);
        assert!(parse_batch_response("application/json", "").is_err());
    }
}
