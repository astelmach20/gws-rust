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

//! Direct REST client shared by the app helpers (`drive`, `docs`, `sheets`,
//! `calendar`, `script`, `chat`, and the per-service helpers).
//!
//! Helpers talk to Google REST endpoints directly (URLs derived from the
//! service's Discovery `rootUrl` + `servicePath`, so endpoint overrides applied
//! to the Discovery document carry over). Every request goes through
//! [`Api::execute`], which is the single place that:
//!
//! - honours `--dry-run` (requests are recorded and printed, never sent),
//! - attaches the bearer token,
//! - delegates retry policy to [`crate::client::send_with_retry`] (the one
//!   function to switch when the executor retry policy changes),
//! - converts non-2xx responses into [`GwsError::Api`] with Google's error
//!   message and reason.

use crate::error::GwsError;
use clap::ArgMatches;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Build a [`GwsError`] for unexpected internal failures.
///
/// Goes through `From<anyhow::Error>` so it stays source-compatible with the
/// core error API regardless of how the `Other` variant is represented.
pub(crate) fn other_err(e: impl Into<anyhow::Error>) -> GwsError {
    GwsError::from(e.into())
}

/// Percent-encode a single path segment per RFC 3986, leaving the unreserved
/// characters (`A-Z a-z 0-9 - . _ ~`) and `@` (a legal `pchar`, common in
/// calendar IDs and `@default`) intact.
///
/// Google IDs routinely contain `-` and `_` (Apps Script IDs, Drive IDs);
/// some endpoints (e.g. `scripts.run`, upstream #842) reject them when they
/// are encoded as `%2D` / `%5F`.
pub(crate) fn encode_segment(s: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b'@');
    // "." and ".." are dot-segments and must never be sent verbatim.
    if s == "." || s == ".." {
        return s.replace('.', "%2E");
    }
    utf8_percent_encode(s, SEGMENT).to_string()
}

/// Request body variants.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    None,
    Json(Value),
    Bytes {
        data: bytes::Bytes,
        content_type: String,
    },
}

/// A fully described HTTP request.
#[derive(Debug, Clone)]
pub(crate) struct ApiRequest {
    pub method: reqwest::Method,
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl ApiRequest {
    fn new(method: reqwest::Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
        }
    }
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::GET, url)
    }
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::POST, url)
    }
    pub fn put(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::PUT, url)
    }
    pub fn patch(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::PATCH, url)
    }
    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::DELETE, url)
    }
    pub fn query(mut self, key: &str, value: impl Into<String>) -> Self {
        self.query.push((key.to_string(), value.into()));
        self
    }
    pub fn query_opt(self, key: &str, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.query(key, v),
            None => self,
        }
    }
    pub fn header(mut self, key: &str, value: impl Into<String>) -> Self {
        self.headers.push((key.to_string(), value.into()));
        self
    }
    pub fn json(mut self, body: Value) -> Self {
        self.body = Body::Json(body);
        self
    }
    pub fn bytes(mut self, data: bytes::Bytes, content_type: &str) -> Self {
        self.body = Body::Bytes {
            data,
            content_type: content_type.to_string(),
        };
        self
    }

    /// JSON description of this request (used for `--dry-run`).
    pub fn describe(&self) -> Value {
        let mut out = json!({
            "method": self.method.as_str(),
            "url": self.url,
        });
        if !self.query.is_empty() {
            let q: Map<String, Value> = self
                .query
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            out["query"] = Value::Object(q);
        }
        match &self.body {
            Body::None => {}
            Body::Json(v) => out["body"] = v.clone(),
            Body::Bytes { data, content_type } => {
                out["body"] = json!({ "contentType": content_type, "bytes": data.len() });
            }
        }
        out
    }

    fn builder(&self, client: &reqwest::Client, token: Option<&str>) -> reqwest::RequestBuilder {
        let mut rb = client.request(self.method.clone(), &self.url);
        if !self.query.is_empty() {
            rb = rb.query(&self.query);
        }
        for (k, v) in &self.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        match &self.body {
            Body::None => {
                if matches!(
                    self.method,
                    reqwest::Method::POST | reqwest::Method::PUT | reqwest::Method::PATCH
                ) {
                    rb = rb.header(reqwest::header::CONTENT_LENGTH, "0");
                }
            }
            Body::Json(v) => rb = rb.json(v),
            Body::Bytes { data, content_type } => {
                rb = rb
                    .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                    .body(data.clone());
            }
        }
        rb
    }
}

/// Result of a paginated listing.
#[derive(Debug, Default)]
pub(crate) struct Page {
    pub items: Vec<Value>,
    /// True when `limit` stopped the listing before all results were read.
    pub truncated: bool,
    /// Resume token, when the cut fell exactly on a page boundary.
    pub next_page_token: Option<String>,
}

impl Page {
    /// Standard JSON envelope for list output. Truncation is always explicit:
    /// `truncated` is present and `nextPageToken` is returned so the caller can
    /// resume. A note is also written to stderr.
    pub fn into_json(self, key: &str) -> Value {
        let count = self.items.len();
        let mut out = json!({ key: self.items, "count": count });
        if self.truncated {
            eprintln!("note: output limited to {count} {key} by --limit; more results exist");
            out["truncated"] = json!(true);
            if let Some(token) = self.next_page_token {
                out["nextPageToken"] = json!(token);
            }
        }
        out
    }
}

/// Where a downloaded body is written.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OutputTarget {
    Stdout,
    File { path: PathBuf, overwrite: bool },
}

impl OutputTarget {
    /// Parse an `--output` value (`-` means stdout). File paths are validated
    /// to stay under the current directory.
    pub fn parse(value: &str, overwrite: bool) -> Result<Self, GwsError> {
        if value == "-" {
            return Ok(Self::Stdout);
        }
        let path = crate::validate::validate_safe_file_path(value, "--output")?;
        Ok(Self::File { path, overwrite })
    }

    pub fn describe(&self) -> Value {
        match self {
            Self::Stdout => json!("-"),
            Self::File { path, .. } => json!(path.display().to_string()),
        }
    }
}

/// Direct REST client for a single helper invocation.
pub(crate) struct Api {
    client: reqwest::Client,
    token: Option<String>,
    dry_run: bool,
    /// Service base URL (`rootUrl + servicePath`), always ending in `/`.
    base: String,
    /// `rootUrl`, always ending in `/`.
    root: String,
    planned: Mutex<Vec<Value>>,
    sanitize: crate::helpers::modelarmor::SanitizeConfig,
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

impl Api {
    /// Build a client for `doc`, acquiring a token for `scopes` unless this is
    /// a dry run (dry runs never need credentials and never send requests).
    pub async fn new(
        doc: &crate::discovery::RestDescription,
        scopes: &[&str],
        dry_run: bool,
        sanitize: &crate::helpers::modelarmor::SanitizeConfig,
    ) -> Result<Self, GwsError> {
        let token = if dry_run {
            None
        } else {
            Some(crate::auth::get_token(scopes).await.map_err(|e| {
                GwsError::Auth(format!(
                    "Failed to obtain credentials for {} (scopes: {}): {e:#}",
                    doc.name,
                    scopes.join(" ")
                ))
            })?)
        };
        let mut api = Self::with_token(doc, token, dry_run)?;
        api.sanitize = sanitize.clone();
        Ok(api)
    }

    /// Build a client with an explicit token (used by tests and callers that
    /// already hold one).
    pub fn with_token(
        doc: &crate::discovery::RestDescription,
        token: Option<String>,
        dry_run: bool,
    ) -> Result<Self, GwsError> {
        if doc.root_url.is_empty() {
            return Err(GwsError::Discovery(format!(
                "Discovery document for '{}' has no rootUrl",
                doc.name
            )));
        }
        let root = with_slash(&doc.root_url);
        let service_path = doc.service_path.trim_start_matches('/');
        let base = if service_path.is_empty() {
            root.clone()
        } else {
            format!("{root}{}", with_slash(service_path))
        };
        Ok(Self {
            client: crate::client::shared_client()?,
            token,
            dry_run,
            base,
            root,
            planned: Mutex::new(Vec::new()),
            sanitize: crate::helpers::modelarmor::SanitizeConfig::default(),
        })
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// The bearer token (absent in dry-run mode).
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// The shared HTTP client.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// `rootUrl + servicePath + path` (path given without leading slash).
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path.trim_start_matches('/'))
    }

    /// `rootUrl + path` for endpoints outside the service path (uploads).
    pub fn root_url(&self, path: &str) -> String {
        format!("{}{}", self.root, path.trim_start_matches('/'))
    }

    fn record(&self, value: Value) -> Result<(), GwsError> {
        self.planned
            .lock()
            .map_err(|_| other_err(anyhow::anyhow!("dry-run plan lock poisoned")))?
            .push(value);
        Ok(())
    }

    /// Record an informational step in the dry-run plan (e.g. a file write).
    pub fn plan_note(&self, value: Value) -> Result<(), GwsError> {
        if self.dry_run {
            self.record(value)?;
        }
        Ok(())
    }

    /// Send a request and return the successful response.
    ///
    /// This is the single choke point for all helper HTTP traffic. It must not
    /// be called in dry-run mode (callers use [`Api::send`] / [`Api::send_raw`]).
    async fn execute(&self, req: &ApiRequest) -> Result<reqwest::Response, GwsError> {
        let token = self.token.as_deref();
        let resp = crate::client::send_with_retry(|| req.builder(&self.client, token))
            .await
            .map_err(|e| {
                other_err(anyhow::anyhow!(
                    "{} {} failed: {e}",
                    req.method,
                    redact_url(&req.url)
                ))
            })?;
        let status = resp.status();
        if status.is_success() || status == reqwest::StatusCode::PERMANENT_REDIRECT {
            return Ok(resp);
        }
        let body = resp.text().await.map_err(|e| {
            other_err(anyhow::anyhow!(
                "HTTP {status} from {} and the error body could not be read: {e}",
                redact_url(&req.url)
            ))
        })?;
        Err(api_error(status.as_u16(), &body))
    }

    /// Send a request and parse the JSON response. An empty 2xx body yields
    /// `Value::Null`. In dry-run mode the request is recorded and `Null` is
    /// returned.
    pub async fn send(&self, req: ApiRequest) -> Result<Value, GwsError> {
        if self.dry_run {
            self.record(req.describe())?;
            return Ok(Value::Null);
        }
        let resp = self.execute(&req).await?;
        let text = resp.text().await.map_err(|e| {
            other_err(anyhow::anyhow!(
                "Failed to read response from {}: {e}",
                redact_url(&req.url)
            ))
        })?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| {
            other_err(anyhow::anyhow!(
                "Response from {} was not valid JSON: {e}",
                redact_url(&req.url)
            ))
        })
    }

    /// Follow `nextPageToken` until exhausted or `limit` items are collected.
    ///
    /// `items_key` is the array field in each page. A missing array on a page
    /// means "no items" (Google omits empty arrays). A repeated page token is
    /// an error rather than an infinite loop.
    pub async fn paginate(
        &self,
        req: ApiRequest,
        items_key: &str,
        limit: Option<usize>,
    ) -> Result<Page, GwsError> {
        let mut page = Page::default();
        let mut token: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        loop {
            let mut this = req.clone();
            if let Some(t) = &token {
                this.query.retain(|(k, _)| k != "pageToken");
                this = this.query("pageToken", t.clone());
            }
            let value = self.send(this).await?;
            if self.dry_run {
                return Ok(page);
            }
            if let Some(items) = value.get(items_key) {
                let arr = items.as_array().ok_or_else(|| {
                    other_err(anyhow::anyhow!(
                        "Expected '{items_key}' to be an array in list response"
                    ))
                })?;
                page.items.extend(arr.iter().cloned());
            }
            let next = value
                .get("nextPageToken")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            if let Some(max) = limit
                && page.items.len() >= max
            {
                // Items beyond `max` on this page cannot be resumed with a page
                // token; only a cut exactly on a page edge is resumable.
                let cut_mid_page = page.items.len() > max;
                page.items.truncate(max);
                page.truncated = cut_mid_page || next.is_some();
                page.next_page_token = if cut_mid_page { None } else { next };
                return Ok(page);
            }
            match next {
                Some(n) => {
                    if !seen.insert(n.clone()) {
                        return Err(other_err(anyhow::anyhow!(
                            "API returned a repeated nextPageToken; aborting pagination"
                        )));
                    }
                    token = Some(n);
                }
                None => return Ok(page),
            }
        }
    }

    /// Stream a response body to `target`. Files are written to a temporary
    /// file in the destination directory and atomically renamed into place;
    /// an existing file is only replaced when `overwrite` is set.
    pub async fn download(
        &self,
        req: ApiRequest,
        target: &OutputTarget,
    ) -> Result<Option<u64>, GwsError> {
        if self.dry_run {
            let mut d = req.describe();
            d["output"] = target.describe();
            self.record(d)?;
            return Ok(None);
        }
        let resp = self.execute(&req).await?;
        write_stream(resp, target).await.map(Some)
    }

    /// Output the helper's result: the dry-run plan in dry-run mode, else
    /// `value` in the requested `--format`, after Model Armor screening when
    /// `--sanitize` is configured.
    pub async fn emit(&self, matches: &ArgMatches, value: &Value) -> Result<(), GwsError> {
        if self.dry_run {
            let planned = self
                .planned
                .lock()
                .map_err(|_| other_err(anyhow::anyhow!("dry-run plan lock poisoned")))?
                .clone();
            return print_value(matches, &json!({ "dry_run": true, "requests": planned }));
        }
        let mut value = value.clone();
        if let Some(template) = self.sanitize.template.as_deref() {
            let text = serde_json::to_string(&value).map_err(other_err)?;
            let verdict = screen(template, &self.sanitize.mode, &text).await?;
            if let Some(obj) = value.as_object_mut() {
                obj.insert("_sanitization".into(), verdict);
            }
        }
        print_value(matches, &value)
    }

    /// Screen text through Model Armor (when configured) before it is written
    /// somewhere other than stdout.
    pub async fn screen_text(&self, text: &str) -> Result<(), GwsError> {
        if !self.dry_run
            && let Some(template) = self.sanitize.template.as_deref()
        {
            screen(template, &self.sanitize.mode, text).await?;
        }
        Ok(())
    }

    /// Output plain text (e.g. a rendered document), screened like [`Api::emit`].
    /// Text output cannot carry an annotation, so warn-mode findings go to stderr.
    pub async fn emit_text(&self, matches: &ArgMatches, text: &str) -> Result<(), GwsError> {
        if self.dry_run {
            return self.emit(matches, &Value::Null).await;
        }
        if let Some(template) = self.sanitize.template.as_deref() {
            screen(template, &self.sanitize.mode, text).await?;
        }
        print_text(text)
    }

    /// Dry-run plan recorded so far (tests).
    #[cfg(test)]
    pub fn planned(&self) -> Vec<Value> {
        self.planned.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Resumable upload (`uploadType=resumable`) of a local file.
    ///
    /// `init` is the session-initiation request (POST/PATCH with JSON
    /// metadata, `uploadType=resumable` in the query). The file is sent in
    /// 8 MiB chunks, each retried independently.
    pub async fn upload_resumable(
        &self,
        init: ApiRequest,
        file: &Path,
        content_type: &str,
    ) -> Result<Value, GwsError> {
        let size = std::fs::metadata(file)
            .map_err(|e| GwsError::Validation(format!("Cannot read '{}': {e}", file.display())))?
            .len();
        let init = init
            .header("X-Upload-Content-Type", content_type)
            .header("X-Upload-Content-Length", size.to_string());
        if self.dry_run {
            let mut d = init.describe();
            d["upload"] = json!({
                "protocol": "resumable",
                "path": file.display().to_string(),
                "bytes": size,
                "contentType": content_type,
            });
            self.record(d)?;
            return Ok(Value::Null);
        }
        let resp = self.execute(&init).await?;
        let session = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                other_err(anyhow::anyhow!(
                    "Upload session response had no Location header"
                ))
            })?
            .to_string();
        ensure_same_origin(&init.url, &session)?;

        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(file)
            .map_err(|e| GwsError::Validation(format!("Cannot open '{}': {e}", file.display())))?;
        let mut offset: u64 = 0;
        loop {
            let remaining = size - offset;
            let len = remaining.min(UPLOAD_CHUNK);
            let mut buf = vec![0u8; usize::try_from(len).map_err(other_err)?];
            f.seek(SeekFrom::Start(offset)).map_err(other_err)?;
            f.read_exact(&mut buf).map_err(|e| {
                other_err(anyhow::anyhow!(
                    "Failed reading '{}' at offset {offset}: {e}",
                    file.display()
                ))
            })?;
            let range = if size == 0 {
                "bytes */0".to_string()
            } else {
                format!("bytes {offset}-{}/{size}", offset + len - 1)
            };
            let put = ApiRequest::put(session.clone())
                .header("Content-Range", range)
                .bytes(bytes::Bytes::from(buf), content_type);
            let resp = self.execute(&put).await?;
            if resp.status() == reqwest::StatusCode::PERMANENT_REDIRECT {
                // 308 Resume Incomplete: `Range: bytes=0-N` says what the server has.
                let received = resp
                    .headers()
                    .get(reqwest::header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_range_end)
                    .map(|end| end + 1)
                    .unwrap_or(0);
                if received <= offset && len > 0 {
                    return Err(other_err(anyhow::anyhow!(
                        "Upload made no progress at byte {offset} of {size}"
                    )));
                }
                offset = received;
                continue;
            }
            let text = resp.text().await.map_err(other_err)?;
            return serde_json::from_str(&text).map_err(|e| {
                other_err(anyhow::anyhow!("Upload response was not valid JSON: {e}"))
            });
        }
    }
}

/// Screen `text` through Model Armor. Fails closed in block mode (a match or
/// a failed screening is an error); in warn mode findings and failures are
/// reported on stderr and returned as the `_sanitization` annotation.
async fn screen(
    template: &str,
    mode: &crate::helpers::modelarmor::SanitizeMode,
    text: &str,
) -> Result<Value, GwsError> {
    use crate::helpers::modelarmor::{SanitizeMode, sanitize_text};
    match sanitize_text(template, text).await {
        Ok(result) => {
            let matched = result.filter_match_state == "MATCH_FOUND";
            let verdict = serde_json::to_value(&result).map_err(other_err)?;
            if matched {
                if *mode == SanitizeMode::Block {
                    return Err(other_err(anyhow::anyhow!(
                        "Content blocked by Model Armor (filterMatchState: MATCH_FOUND)"
                    )));
                }
                eprintln!(
                    "warning: Model Armor flagged this content (filterMatchState: MATCH_FOUND)"
                );
            }
            Ok(verdict)
        }
        Err(e) => {
            if *mode == SanitizeMode::Block {
                return Err(other_err(anyhow::anyhow!(
                    "Model Armor screening failed and --sanitize is in block mode; output suppressed: {e}"
                )));
            }
            eprintln!("warning: Model Armor screening failed; output is unscreened: {e}");
            Ok(json!({ "error": e.to_string() }))
        }
    }
}

/// Upload chunk size: 8 MiB (must be a multiple of 256 KiB).
const UPLOAD_CHUNK: u64 = 8 * 1024 * 1024;

fn parse_range_end(v: &str) -> Option<u64> {
    v.trim()
        .strip_prefix("bytes=")?
        .split('-')
        .nth(1)?
        .parse()
        .ok()
}

/// The upload session URL returned by the server must stay on the same
/// origin as the initiating request, otherwise the file contents (and on some
/// servers the credentials) would be sent elsewhere.
fn ensure_same_origin(a: &str, b: &str) -> Result<(), GwsError> {
    let pa = reqwest::Url::parse(a).map_err(other_err)?;
    let pb = reqwest::Url::parse(b)
        .map_err(|e| other_err(anyhow::anyhow!("Invalid upload session URL: {e}")))?;
    if pa.origin() != pb.origin() {
        return Err(other_err(anyhow::anyhow!(
            "Upload session URL origin {} does not match request origin {}",
            pb.origin().ascii_serialization(),
            pa.origin().ascii_serialization()
        )));
    }
    Ok(())
}

/// Strip the query string from a URL for error messages.
fn redact_url(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// Convert an error response into [`GwsError::Api`], using Google's JSON
/// error format when present.
pub(crate) fn api_error(status: u16, body: &str) -> GwsError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let err = parsed.as_ref().and_then(|v| v.get("error"));
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                format!("HTTP {status} with an empty response body")
            } else {
                trimmed.chars().take(2000).collect()
            }
        });
    let reason = err
        .and_then(|e| e.get("errors"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|e| e.get("reason"))
        .and_then(Value::as_str)
        .or_else(|| err.and_then(|e| e.get("status")).and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let enable_url = if reason == "accessNotConfigured" || reason == "SERVICE_DISABLED" {
        crate::executor::extract_enable_url(&message)
    } else {
        None
    };
    GwsError::Api {
        code: status,
        message,
        reason,
        enable_url,
    }
}

async fn write_stream(resp: reqwest::Response, target: &OutputTarget) -> Result<u64, GwsError> {
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    match target {
        OutputTarget::Stdout => {
            use tokio::io::AsyncWriteExt;
            let mut out = tokio::io::stdout();
            while let Some(chunk) = stream.next().await {
                let chunk =
                    chunk.map_err(|e| other_err(anyhow::anyhow!("Download interrupted: {e}")))?;
                out.write_all(&chunk)
                    .await
                    .map_err(|e| other_err(anyhow::anyhow!("Failed writing to stdout: {e}")))?;
                written += chunk.len() as u64;
            }
            out.flush()
                .await
                .map_err(|e| other_err(anyhow::anyhow!("Failed flushing stdout: {e}")))?;
        }
        OutputTarget::File { path, overwrite } => {
            if !overwrite && path.exists() {
                return Err(GwsError::Validation(format!(
                    "'{}' already exists; pass --overwrite to replace it",
                    path.display()
                )));
            }
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            std::fs::create_dir_all(dir).map_err(|e| {
                other_err(anyhow::anyhow!(
                    "Failed to create directory '{}': {e}",
                    dir.display()
                ))
            })?;
            let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| {
                other_err(anyhow::anyhow!(
                    "Failed to create temporary file in '{}': {e}",
                    dir.display()
                ))
            })?;
            while let Some(chunk) = stream.next().await {
                let chunk =
                    chunk.map_err(|e| other_err(anyhow::anyhow!("Download interrupted: {e}")))?;
                tmp.write_all(&chunk).map_err(|e| {
                    other_err(anyhow::anyhow!("Failed writing '{}': {e}", path.display()))
                })?;
                written += chunk.len() as u64;
            }
            tmp.as_file().sync_all().map_err(|e| {
                other_err(anyhow::anyhow!("Failed syncing '{}': {e}", path.display()))
            })?;
            persist(tmp, path, *overwrite)?;
        }
    }
    Ok(written)
}

/// Move a finished temp file into place.
fn persist(tmp: tempfile::NamedTempFile, path: &Path, overwrite: bool) -> Result<(), GwsError> {
    let result = if overwrite {
        tmp.persist(path).map(|_| ())
    } else {
        tmp.persist_noclobber(path).map(|_| ())
    };
    result.map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            GwsError::Validation(format!(
                "'{}' already exists; pass --overwrite to replace it",
                path.display()
            ))
        } else {
            other_err(anyhow::anyhow!(
                "Failed to write '{}': {}",
                path.display(),
                e.error
            ))
        }
    })
}

/// Write bytes to a local file atomically (same overwrite rules as downloads).
pub(crate) fn write_file_atomic(path: &Path, data: &[u8], overwrite: bool) -> Result<(), GwsError> {
    if !overwrite && path.exists() {
        return Err(GwsError::Validation(format!(
            "'{}' already exists; pass --overwrite to replace it",
            path.display()
        )));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| {
        other_err(anyhow::anyhow!(
            "Failed to create directory '{}': {e}",
            dir.display()
        ))
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(other_err)?;
    tmp.write_all(data)
        .map_err(|e| other_err(anyhow::anyhow!("Failed writing '{}': {e}", path.display())))?;
    persist(tmp, path, overwrite)
}

/// Turn a remote (untrusted) file name into a safe single path component.
///
/// Path separators, control characters and characters that are invalid on
/// Windows are replaced with `_`; leading dots are stripped so the result can
/// never be `.`/`..` or a hidden file. Falls back to `fallback` when nothing
/// usable remains.
pub(crate) fn safe_filename(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    let truncated: String = cleaned.chars().take(200).collect();
    if truncated.is_empty() {
        fallback.to_string()
    } else {
        truncated
    }
}

/// Resolve the global `--format` flag. Unknown formats are an error.
pub(crate) fn output_format(
    matches: &ArgMatches,
) -> Result<crate::formatter::OutputFormat, GwsError> {
    match matches.try_get_one::<String>("format").ok().flatten() {
        None => Ok(crate::formatter::OutputFormat::default()),
        Some(s) => crate::formatter::OutputFormat::parse(s).map_err(|bad| {
            GwsError::Validation(format!(
                "Unknown --format '{bad}' (valid: json, table, yaml, csv)"
            ))
        }),
    }
}

/// Print `value` in the requested output format.
pub(crate) fn print_value(matches: &ArgMatches, value: &Value) -> Result<(), GwsError> {
    let format = output_format(matches)?;
    print_text(&crate::formatter::format_value(value, &format)?)
}

/// Print text to stdout through the shared writer (broken pipes end quietly).
pub(crate) fn print_text(text: &str) -> Result<(), GwsError> {
    crate::output::emit(text)
}

/// Whether the global `--dry-run` flag is set.
pub(crate) fn dry_run(matches: &ArgMatches) -> bool {
    matches
        .try_get_one::<bool>("dry-run")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(false)
}

/// Fetch a required string argument without panicking.
pub(crate) fn required<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a str, GwsError> {
    matches
        .try_get_one::<String>(name)
        .map_err(|e| other_err(anyhow::anyhow!("argument '{name}' is not defined: {e}")))?
        .map(String::as_str)
        .ok_or_else(|| GwsError::Validation(format!("--{name} is required")))
}

/// Fetch an optional string argument.
pub(crate) fn optional<'a>(matches: &'a ArgMatches, name: &str) -> Option<&'a str> {
    matches
        .try_get_one::<String>(name)
        .ok()
        .flatten()
        .map(String::as_str)
}

/// Fetch all values of a repeatable argument.
pub(crate) fn many(matches: &ArgMatches, name: &str) -> Vec<String> {
    matches
        .try_get_many::<String>(name)
        .ok()
        .flatten()
        .map(|v| v.cloned().collect())
        .unwrap_or_default()
}

/// Fetch a boolean flag.
pub(crate) fn flag(matches: &ArgMatches, name: &str) -> bool {
    matches
        .try_get_one::<bool>(name)
        .ok()
        .flatten()
        .copied()
        .unwrap_or(false)
}

/// Parse an optional `--limit`-style positive integer argument.
pub(crate) fn limit(matches: &ArgMatches, name: &str) -> Result<Option<usize>, GwsError> {
    optional(matches, name)
        .map(|s| {
            s.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| {
                GwsError::Validation(format!("--{name} must be a positive integer, got '{s}'"))
            })
        })
        .transpose()
}

/// Read text from a file path, or stdin when the path is `-`.
pub(crate) fn read_text_input(path: &str, flag_name: &str) -> Result<String, GwsError> {
    if path == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| {
            GwsError::Validation(format!("Failed to read {flag_name} from stdin: {e}"))
        })?;
        return Ok(s);
    }
    let safe = crate::validate::validate_safe_file_path(path, flag_name)?;
    std::fs::read_to_string(&safe)
        .map_err(|e| GwsError::Validation(format!("Failed to read {flag_name} '{path}': {e}")))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A Discovery document whose rootUrl points at `root`.
    pub fn doc(name: &str, root: &str, service_path: &str) -> crate::discovery::RestDescription {
        crate::discovery::RestDescription {
            name: name.to_string(),
            root_url: format!("{root}/"),
            service_path: service_path.to_string(),
            ..Default::default()
        }
    }

    pub fn api(root: &str, service_path: &str) -> Api {
        Api::with_token(
            &doc("test", root, service_path),
            Some("test-token".into()),
            false,
        )
        .expect("api")
    }

    pub fn dry_api(service_path: &str) -> Api {
        Api::with_token(
            &doc("test", "https://example.googleapis.com", service_path),
            None,
            true,
        )
        .expect("api")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::test_support::*;
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn encode_segment_keeps_unreserved() {
        assert_eq!(encode_segment("1-ab_C.d~e"), "1-ab_C.d~e");
        assert_eq!(encode_segment("a/b?c#d e"), "a%2Fb%3Fc%23d%20e");
        assert_eq!(encode_segment(".."), "%2E%2E");
        assert_eq!(encode_segment("a:b"), "a%3Ab");
        assert_eq!(encode_segment("ann@example.com"), "ann@example.com");
    }

    #[test]
    fn api_error_parses_google_format() {
        let body = r#"{"error":{"code":404,"message":"File not found: x","errors":[{"reason":"notFound"}]}}"#;
        match api_error(404, body) {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(code, 404);
                assert_eq!(message, "File not found: x");
                assert_eq!(reason, "notFound");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_non_json_and_empty() {
        match api_error(502, "") {
            GwsError::Api { message, .. } => assert!(message.contains("empty response body")),
            other => panic!("unexpected {other:?}"),
        }
        match api_error(500, "<html>boom</html>") {
            GwsError::Api { message, .. } => assert_eq!(message, "<html>boom</html>"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn api_error_uses_status_for_v2_style_errors() {
        let body = r#"{"error":{"code":403,"message":"denied","status":"PERMISSION_DENIED"}}"#;
        match api_error(403, body) {
            GwsError::Api { reason, .. } => assert_eq!(reason, "PERMISSION_DENIED"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn url_building() {
        let api = api("https://x.googleapis.com", "drive/v3/");
        assert_eq!(api.url("files"), "https://x.googleapis.com/drive/v3/files");
        assert_eq!(
            api.root_url("upload/drive/v3/files"),
            "https://x.googleapis.com/upload/drive/v3/files"
        );
        let api = api_nopath();
        assert_eq!(api.url("/v1/x"), "https://x.googleapis.com/v1/x");
    }

    fn api_nopath() -> Api {
        api("https://x.googleapis.com", "")
    }

    #[tokio::test]
    async fn send_attaches_token_and_parses_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/svc/items"))
            .and(header("authorization", "Bearer test-token"))
            .and(query_param("a", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "svc/");
        let v = api
            .send(ApiRequest::get(api.url("items")).query("a", "1"))
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn send_empty_body_is_null() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = api.send(ApiRequest::delete(api.url("x"))).await.unwrap();
        assert!(v.is_null());
    }

    #[tokio::test]
    async fn send_error_is_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": 403, "message": "nope", "errors": [{"reason": "forbidden"}]}
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let err = api.send(ApiRequest::get(api.url("x"))).await.unwrap_err();
        match err {
            GwsError::Api { code, reason, .. } => {
                assert_eq!(code, 403);
                assert_eq!(reason, "forbidden");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn dry_run_records_without_sending() {
        let api = dry_api("svc/");
        let v = api
            .send(ApiRequest::post(api.url("x")).json(json!({"a": 1})))
            .await
            .unwrap();
        assert!(v.is_null());
        let plan = api.planned();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0]["method"], "POST");
        assert_eq!(plan[0]["body"]["a"], 1);
    }

    #[tokio::test]
    async fn paginate_follows_all_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [3]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [1, 2], "nextPageToken": "p2"})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let page = api
            .paginate(ApiRequest::get(api.url("x")), "items", None)
            .await
            .unwrap();
        assert_eq!(page.items, vec![json!(1), json!(2), json!(3)]);
        assert!(page.next_page_token.is_none());
    }

    #[tokio::test]
    async fn paginate_limit_is_explicit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [1, 2], "nextPageToken": "p2"})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let page = api
            .paginate(ApiRequest::get(api.url("x")), "items", Some(2))
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.next_page_token.as_deref(), Some("p2"));
        let out = page.into_json("items");
        assert_eq!(out["truncated"], true);
        assert_eq!(out["nextPageToken"], "p2");
    }

    #[tokio::test]
    async fn paginate_repeated_token_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [1], "nextPageToken": "same"})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert!(
            api.paginate(ApiRequest::get(api.url("x")), "items", None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn download_writes_file_and_refuses_overwrite() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.bin");
        let target = OutputTarget::File {
            path: path.clone(),
            overwrite: false,
        };
        let n = api
            .download(ApiRequest::get(api.url("x")), &target)
            .await
            .unwrap();
        assert_eq!(n, Some(5));
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        let err = api
            .download(ApiRequest::get(api.url("x")), &target)
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)));
    }

    #[tokio::test]
    async fn resumable_upload_sends_chunks() {
        let server = MockServer::start().await;
        let session = format!("{}/upload/session/abc", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/files"))
            .and(query_param("uploadType", "resumable"))
            .and(header("x-upload-content-length", "5"))
            .respond_with(ResponseTemplate::new(200).insert_header("location", session.as_str()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload/session/abc"))
            .and(header("content-range", "bytes 0-4/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "F1"})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let init = ApiRequest::post(api.root_url("upload/files"))
            .query("uploadType", "resumable")
            .json(json!({"name": "a.txt"}));
        let v = api
            .upload_resumable(init, &file, "text/plain")
            .await
            .unwrap();
        assert_eq!(v["id"], "F1");
    }

    #[tokio::test]
    async fn resumable_upload_rejects_foreign_session_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("location", "https://evil.example/upload"),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let init = ApiRequest::post(api.root_url("upload/files")).json(json!({}));
        assert!(
            api.upload_resumable(init, &file, "text/plain")
                .await
                .is_err()
        );
    }

    #[test]
    fn safe_filename_strips_dangerous_parts() {
        assert_eq!(safe_filename("../../etc/passwd", "x"), "_.._etc_passwd");
        assert_eq!(safe_filename("..", "x"), "x");
        assert_eq!(safe_filename(".bashrc", "x"), "bashrc");
        assert_eq!(safe_filename("a\nb:c", "x"), "a_b_c");
        assert_eq!(safe_filename("Report Q1.pdf", "x"), "Report Q1.pdf");
    }

    #[test]
    fn parse_range_end_works() {
        assert_eq!(parse_range_end("bytes=0-8388607"), Some(8_388_607));
        assert_eq!(parse_range_end("garbage"), None);
    }

    #[test]
    fn write_file_atomic_respects_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        write_file_atomic(&p, b"a", false).unwrap();
        assert!(write_file_atomic(&p, b"b", false).is_err());
        write_file_atomic(&p, b"c", true).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"c");
    }
}
