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
//! [`Api::execute`], which honours `--dry-run` (requests are recorded and
//! printed, never sent) and otherwise hands the request to the shared
//! [`crate::transport::Transport`]: endpoint validation, the bearer token and
//! its refresh on 401, core retries (non-idempotent requests are sent once),
//! and Google error mapping all happen there.

use crate::error::GwsError;
use clap::ArgMatches;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gws_rust_core::client::Idempotency;

use crate::transport::Transport;

/// Build a [`GwsError`] for unexpected internal failures.
///
/// Goes through `From<anyhow::Error>` so it stays source-compatible with the
/// core error API regardless of how the `Other` variant is represented.
pub(crate) fn other_err(e: impl Into<anyhow::Error>) -> GwsError {
    GwsError::from(e.into())
}

/// Request body variants.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    None,
    Json(Value),
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
            out["query_params"] = Value::Object(q);
        }
        match &self.body {
            Body::None => {}
            Body::Json(v) => out["body"] = v.clone(),
        }
        out
    }

    /// Apply query, headers and body to a request builder from the transport.
    fn apply(&self, mut rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if !self.query.is_empty() {
            rb = rb.query(&self.query);
        }
        for (k, v) in &self.headers {
            rb = rb.header(k.as_str(), v.as_str());
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
            tracing::warn!("output limited to {count} {key} by --limit; more results exist");
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
    /// by the `GWSR_RESTRICT_PATHS` policy (any path by default; `cwd` confines
    /// them to the current directory).
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
    /// `None` in dry-run mode: nothing is sent, so no credentials are loaded.
    transport: Option<Transport>,
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
    /// Build a client for `doc`, acquiring credentials for `scopes` unless
    /// this is a dry run (dry runs never need credentials and never send
    /// requests).
    pub async fn new(
        doc: &crate::discovery::RestDescription,
        scopes: &[&str],
        dry_run: bool,
        sanitize: &crate::helpers::modelarmor::SanitizeConfig,
    ) -> Result<Self, GwsError> {
        let transport = if dry_run {
            None
        } else {
            Some(Transport::for_scopes(scopes).await?)
        };
        let mut api = Self::with_transport(doc, transport)?;
        api.sanitize = sanitize.clone();
        Ok(api)
    }

    /// Build a client from an existing transport (`None` = dry run).
    pub fn with_transport(
        doc: &crate::discovery::RestDescription,
        transport: Option<Transport>,
    ) -> Result<Self, GwsError> {
        // Dry runs show the URLs a real run would use, including the
        // GWSR_API_BASE_URL override.
        let env_endpoints;
        let endpoints = match &transport {
            Some(t) => t.endpoints(),
            None => {
                env_endpoints = crate::env::get()?.endpoint_policy();
                &env_endpoints
            }
        };
        let override_base = endpoints.override_base();
        let root = doc.api_root(override_base)?.to_string();
        let base = doc.service_base(override_base)?;
        Ok(Self {
            transport,
            base: with_slash(&base),
            root: with_slash(&root),
            planned: Mutex::new(Vec::new()),
            sanitize: crate::helpers::modelarmor::SanitizeConfig::default(),
        })
    }

    pub fn is_dry_run(&self) -> bool {
        self.transport.is_none()
    }

    /// The transport (absent in dry-run mode).
    pub fn transport(&self) -> Option<&Transport> {
        self.transport.as_ref()
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
        if self.is_dry_run() {
            self.record(value)?;
        }
        Ok(())
    }

    /// Send a request and return the successful response (2xx, or 308 for
    /// resumable-upload progress). Must not be called in dry-run mode.
    async fn execute(&self, req: &ApiRequest) -> Result<reqwest::Response, GwsError> {
        let transport = self.transport.as_ref().ok_or_else(|| {
            GwsError::other("internal error: a dry-run request reached the transport")
        })?;
        let context = format!("{} {}", req.method, redact_url(&req.url));
        transport
            .send_ok(
                req.method.clone(),
                &req.url,
                // GET/PUT/DELETE may be retried; POST/PATCH are sent once.
                Idempotency::FromMethod,
                &context,
                Some(reqwest::StatusCode::PERMANENT_REDIRECT),
                |rb| req.apply(rb),
            )
            .await
    }

    fn idle_timeout(&self) -> Option<std::time::Duration> {
        self.transport
            .as_ref()
            .and_then(|t| t.retry.response_timeout)
    }

    /// Send a request and parse the JSON response. An empty 2xx body yields
    /// `Value::Null`. In dry-run mode the request is recorded and `Null` is
    /// returned.
    pub async fn send(&self, req: ApiRequest) -> Result<Value, GwsError> {
        if self.is_dry_run() {
            self.record(req.describe())?;
            return Ok(Value::Null);
        }
        let resp = self.execute(&req).await?;
        let raw = crate::transport::read_body(resp, self.idle_timeout())
            .await
            .map_err(|e| {
                other_err(anyhow::anyhow!(
                    "Failed to read response from {}: {e}",
                    redact_url(&req.url)
                ))
            })?;
        if raw.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&raw).map_err(|e| {
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
            if self.is_dry_run() {
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
        if self.is_dry_run() {
            let mut d = req.describe();
            d["output"] = target.describe();
            self.record(d)?;
            return Ok(None);
        }
        let resp = self.execute(&req).await?;
        write_stream(resp, target, self.idle_timeout())
            .await
            .map(Some)
    }

    /// Output the helper's result: the dry-run plan in dry-run mode, else
    /// `value` in the requested `--format`, after Model Armor screening when
    /// `--sanitize` is configured.
    pub async fn emit(&self, matches: &ArgMatches, value: &Value) -> Result<(), GwsError> {
        if self.is_dry_run() {
            let planned = self
                .planned
                .lock()
                .map_err(|_| other_err(anyhow::anyhow!("dry-run plan lock poisoned")))?
                .clone();
            return print_value(matches, &json!({ "dry_run": true, "requests": planned }));
        }
        use crate::helpers::modelarmor::{require_pass, sanitize_value};
        let value = require_pass(sanitize_value(&self.sanitize, value.clone()).await?)?;
        print_value(matches, &value)
    }

    /// Screen text through Model Armor (when configured) before it is written
    /// somewhere other than stdout. A block-mode match is an error.
    pub async fn screen_text(&self, text: &str) -> Result<(), GwsError> {
        if !self.is_dry_run() {
            use crate::helpers::modelarmor::{require_pass, sanitize_value};
            require_pass(sanitize_value(&self.sanitize, Value::String(text.to_string())).await?)?;
        }
        Ok(())
    }

    /// Output plain text (e.g. a rendered document), screened like [`Api::emit`].
    pub async fn emit_text(&self, matches: &ArgMatches, text: &str) -> Result<(), GwsError> {
        if self.is_dry_run() {
            return self.emit(matches, &Value::Null).await;
        }
        self.screen_text(text).await?;
        print_text(text)
    }

    /// Dry-run plan recorded so far (tests).
    #[cfg(test)]
    pub fn planned(&self) -> Vec<Value> {
        self.planned.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Resumable upload (`uploadType=resumable`) of a local file.
    ///
    /// `init` is the session-initiation request (POST/PATCH with optional
    /// JSON metadata, `uploadType=resumable` in the query, no extra headers).
    /// The upload itself is [`crate::executor::upload::resumable_upload`], the
    /// same code the generated API methods use: 8 MiB chunks, resumed from
    /// what the server persisted after a failed chunk, with the session URL
    /// required to stay on the request's origin.
    pub async fn upload_resumable(
        &self,
        init: ApiRequest,
        file: &Path,
        content_type: &str,
    ) -> Result<Value, GwsError> {
        if !init.headers.is_empty() {
            return Err(GwsError::other(
                "internal error: a resumable upload session request cannot carry custom headers",
            ));
        }
        let size = std::fs::metadata(file)
            .map_err(|e| GwsError::Validation(format!("Cannot read '{}': {e}", file.display())))?
            .len();
        if self.is_dry_run() {
            let mut d = init
                .clone()
                .header("X-Upload-Content-Type", content_type)
                .header("X-Upload-Content-Length", size.to_string())
                .describe();
            d["upload"] = json!({
                "protocol": "resumable",
                "path": file.display().to_string(),
                "bytes": size,
                "contentType": content_type,
            });
            self.record(d)?;
            return Ok(Value::Null);
        }
        let transport = self.transport.as_ref().ok_or_else(|| {
            GwsError::other("internal error: a dry-run request reached the transport")
        })?;
        let metadata = match init.body {
            Body::Json(v) => Some(v),
            Body::None => None,
        };
        let context = format!("upload to {}", redact_url(&init.url));
        let data = crate::executor::upload::UploadData {
            path: file.display().to_string(),
            size,
        };
        let sent = crate::executor::upload::resumable_upload(
            transport,
            crate::executor::upload::Resumable {
                method: init.method.clone(),
                url: &init.url,
                query: &init.query,
                metadata: &metadata,
                data: &data,
                media_mime: content_type,
                chunk_size: crate::executor::upload::DEFAULT_CHUNK_SIZE,
            },
        )
        .await
        .map_err(|e| crate::transport::errors::with_context(e, &context))?;
        if !sent.response.status().is_success() {
            return Err(transport.error_for(sent, &context).await);
        }
        crate::transport::json_body(sent.response, self.idle_timeout(), &context).await
    }
}

/// Strip the query string from a URL for error messages.
fn redact_url(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

async fn write_stream(
    resp: reqwest::Response,
    target: &OutputTarget,
    idle: Option<std::time::Duration>,
) -> Result<u64, GwsError> {
    match target {
        OutputTarget::Stdout => {
            use tokio::io::AsyncWriteExt;
            let expected = resp.content_length();
            let mut stream = resp.bytes_stream();
            let mut written: u64 = 0;
            let mut out = tokio::io::stdout();
            // Same idle limit and error class as a download to a file.
            while let Some(chunk) = crate::transport::next_chunk(&mut stream, idle).await? {
                out.write_all(&chunk)
                    .await
                    .map_err(|e| other_err(anyhow::anyhow!("Failed writing to stdout: {e}")))?;
                written += chunk.len() as u64;
            }
            out.flush()
                .await
                .map_err(|e| other_err(anyhow::anyhow!("Failed flushing stdout: {e}")))?;
            crate::executor::download::check_length(expected, written)?;
            Ok(written)
        }
        OutputTarget::File { path, overwrite } => {
            crate::executor::download::save_stream(resp, path, *overwrite, idle).await
        }
    }
}

/// Longest name [`safe_filename`] returns, in UTF-8 bytes.
pub(crate) const SAFE_FILENAME_MAX_BYTES: usize = 200;

/// Turn a remote (untrusted) file name into a safe single path component.
///
/// Path separators, control characters and characters that are invalid on
/// Windows are replaced with `_`; leading dots are stripped so the result can
/// never be `.`/`..` or a hidden file. Falls back to `fallback` when nothing
/// usable remains.
///
/// The result is at most [`SAFE_FILENAME_MAX_BYTES`] UTF-8 bytes (cut on a
/// character boundary). File systems limit a name to 255 bytes (ext4) or 255
/// UTF-16 units (APFS, NTFS), and UTF-8 is never shorter than either; the
/// headroom leaves room for callers to add an extension or an ID prefix.
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
    let mut truncated = String::new();
    for c in cleaned.chars() {
        if truncated.len() + c.len_utf8() > SAFE_FILENAME_MAX_BYTES {
            break;
        }
        truncated.push(c);
    }
    if truncated.is_empty() {
        fallback.to_string()
    } else {
        truncated
    }
}

/// The output format: `--format` > `GWSR_FORMAT` > config file > JSON
/// (clap has already rejected unknown `--format` values).
pub(crate) fn output_format(
    matches: &ArgMatches,
) -> Result<crate::formatter::OutputFormat, GwsError> {
    crate::formatter::OutputFormat::from_matches(matches)
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

/// A request description for a `--dry-run` plan, in the same shape as
/// [`ApiRequest::describe`].
pub(crate) fn dry_run_request(
    method: &str,
    url: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Value {
    let mut out = json!({ "method": method, "url": url });
    if !query.is_empty() {
        let q: Map<String, Value> = query
            .iter()
            .map(|(k, v)| ((*k).to_string(), Value::String(v.clone())))
            .collect();
        out["query_params"] = Value::Object(q);
    }
    if let Some(b) = body {
        out["body"] = b.clone();
    }
    out
}

/// Print a dry-run plan (`{"dry_run": true, "requests": [...]}`) in the
/// requested output format.
pub(crate) fn print_dry_run(matches: &ArgMatches, requests: Vec<Value>) -> Result<(), GwsError> {
    print_value(matches, &json!({ "dry_run": true, "requests": requests }))
}

/// Parse an optional `--limit`-style positive integer argument.
pub(crate) fn limit(matches: &ArgMatches, name: &str) -> Result<Option<usize>, GwsError> {
    crate::args::optional(matches, name)?
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
        Api::with_transport(
            &doc("test", root, service_path),
            Some(Transport::for_test(root)),
        )
        .expect("api")
    }

    pub fn dry_api(service_path: &str) -> Api {
        Api::with_transport(
            &doc("test", "https://example.googleapis.com", service_path),
            None,
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
    fn dry_run_request_matches_describe() {
        let req = ApiRequest::post("https://x/y")
            .query("a", "b")
            .json(json!({"k": 1}));
        let v = dry_run_request(
            "POST",
            "https://x/y",
            &[("a", "b".to_string())],
            Some(&json!({"k": 1})),
        );
        assert_eq!(v, req.describe());
        assert_eq!(v["query_params"]["a"], "b");
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
    async fn resumable_upload_resumes_after_a_failed_chunk() {
        let server = MockServer::start().await;
        let session = format!("{}/upload/session/abc", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/files"))
            .respond_with(ResponseTemplate::new(200).insert_header("location", session.as_str()))
            .mount(&server)
            .await;
        // The first chunk attempt fails with a 503 ...
        Mock::given(method("PUT"))
            .and(header("content-range", "bytes 0-4/5"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        // ... the status query says nothing was persisted ...
        Mock::given(method("PUT"))
            .and(header("content-range", "bytes */5"))
            .respond_with(ResponseTemplate::new(308))
            .mount(&server)
            .await;
        // ... and the retried chunk completes the upload.
        Mock::given(method("PUT"))
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
    async fn resumable_upload_maps_api_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": 403, "message": "nope", "errors": [{"reason": "forbidden"}]}
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let init = ApiRequest::post(api.root_url("upload/files")).json(json!({}));
        let err = api
            .upload_resumable(init, &file, "text/plain")
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 403, .. }), "{err:?}");
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
    fn safe_filename_limits_bytes_not_chars() {
        assert_eq!(safe_filename(&"a".repeat(300), "x"), "a".repeat(200));
        // 4-byte characters: 50 fit in 200 bytes, never a split character.
        let emoji = safe_filename(&"\u{1F4C4}".repeat(150), "x");
        assert_eq!(emoji, "\u{1F4C4}".repeat(50));
        let cjk = safe_filename(&"文".repeat(100), "x");
        assert_eq!(cjk, "文".repeat(66));
        assert!(cjk.len() <= SAFE_FILENAME_MAX_BYTES);
    }
}
