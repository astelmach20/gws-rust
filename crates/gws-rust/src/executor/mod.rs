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

//! API request execution.
//!
//! [`execute`] turns a Discovery method plus user input into HTTP requests:
//! input validation ([`input`], [`body_schema`]), URL construction ([`url`]),
//! the credential-attaching transport with retries and token refresh
//! ([`transport`]), uploads ([`upload`]), pagination ([`pagination`]),
//! long-running operations ([`operation`]), downloads ([`download`]),
//! Model Armor screening ([`sanitize`]) and the destructive-operation gate
//! ([`safety`]). [`batch`] implements `gwsr batch`, and [`options`] maps CLI
//! flags onto [`Invocation`].

pub mod batch;
mod body_schema;
mod download;
mod errors;
mod input;
mod operation;
pub mod options;
mod output;
mod pagination;
mod safety;
mod sanitize;
mod transport;
mod upload;
mod url;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Duration;

use reqwest::Method;
use serde_json::{Map, Value, json};

use gws_rust_core::client::{EndpointPolicy, Idempotency, RetryPolicy, Sent};

pub use download::OutputTarget;
pub use errors::extract_enable_url;
pub use operation::WaitConfig;
pub use safety::ConfirmPolicy;
pub use transport::Credentials;
pub use upload::UploadMode;

use crate::discovery::{RestDescription, RestMethod};
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use crate::helpers::modelarmor::{SanitizeConfig, SanitizeMode};
use errors::{error_from_response, other};
use output::Emitter;
use transport::{Transport, read_body};
use upload::UploadData;
use url::{UrlTarget, build_url};

/// Tracks what authentication method was used for the request.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthMethod {
    /// OAuth2 bearer token.
    OAuth,
    /// No authentication was provided.
    None,
}

/// Source for media upload content: a file on disk or in-memory bytes.
pub enum UploadSource<'a> {
    /// Stream from a file on disk. The content type is explicit, inferred
    /// from the extension, or taken from metadata `mimeType`.
    File {
        path: &'a str,
        content_type: Option<&'a str>,
    },
    /// Upload from in-memory bytes with an explicit content type.
    Bytes {
        data: &'a [u8],
        content_type: &'a str,
    },
}

/// Whether `method` paginates (takes a `pageToken` in the query or body).
pub fn paginates(doc: &RestDescription, method: &RestMethod) -> bool {
    pagination::token_location(doc, method).is_some()
}

/// Whether `method` advertises the resumable upload protocol.
pub fn supports_resumable_upload(method: &RestMethod) -> bool {
    url::resumable_upload_path(method).is_some()
}

/// Whether `method` is destructive (see [`safety`]).
pub fn is_destructive(method: &RestMethod) -> bool {
    safety::is_destructive(method)
}

/// Configuration for auto-pagination.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Whether to auto-paginate through all pages.
    pub page_all: bool,
    /// Maximum number of pages to fetch; `0` means unlimited (the default).
    pub page_limit: u32,
    /// Delay between page fetches in milliseconds.
    pub page_delay_ms: u64,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            page_all: false,
            page_limit: 0,
            page_delay_ms: 100,
        }
    }
}

/// `--page-items`: emit one line per item instead of per page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemsMode {
    /// Pick the list field from the response schema.
    Auto,
    /// Use this field.
    Field(String),
}

/// Behavioural options for [`execute`].
#[derive(Debug, Clone)]
pub struct ExecOptions {
    pub dry_run: bool,
    /// `--fields`: partial-response field mask.
    pub fields: Option<String>,
    pub page_items: Option<ItemsMode>,
    /// `--wait`: poll long-running operations until done.
    pub wait: Option<WaitConfig>,
    pub retry: RetryPolicy,
    pub endpoints: EndpointPolicy,
    /// Send `x-goog-user-project` from ADC (disable with `--no-quota-project`).
    pub quota_project: bool,
    pub upload_mode: UploadMode,
    pub upload_chunk_size: usize,
    /// `--decode-field`: base64 field to decode into `--output`.
    pub decode_field: Option<String>,
    pub allow_unknown_params: bool,
    pub allow_unknown_fields: bool,
    /// `--yes`: skip the destructive-operation confirmation.
    pub assume_yes: bool,
    pub confirm: ConfirmPolicy,
    /// Whether a human can answer a confirmation prompt.
    pub interactive: bool,
}

impl ExecOptions {
    /// Defaults with environment-driven settings (`GWSR_TIMEOUT`,
    /// `GWSR_API_BASE_URL`, `GWSR_CONFIRM_DESTRUCTIVE`, `GWSR_NO_QUOTA_PROJECT`).
    pub fn from_env() -> Result<Self, GwsError> {
        Ok(Self {
            dry_run: false,
            fields: None,
            page_items: None,
            wait: None,
            retry: RetryPolicy::from_env()?,
            endpoints: EndpointPolicy::from_env()?,
            quota_project: !options::env_flag(options::NO_QUOTA_PROJECT_ENV)?,
            upload_mode: UploadMode::Auto,
            upload_chunk_size: upload::DEFAULT_CHUNK_SIZE,
            decode_field: None,
            allow_unknown_params: false,
            allow_unknown_fields: false,
            assume_yes: false,
            confirm: ConfirmPolicy::from_env()?,
            interactive: safety::is_interactive(),
        })
    }

    fn idle_timeout(&self) -> Option<Duration> {
        self.retry.response_timeout
    }
}

/// Everything needed to execute one method call.
pub struct Invocation<'a> {
    pub doc: &'a RestDescription,
    pub method: &'a RestMethod,
    pub params: Map<String, Value>,
    pub body: Option<Value>,
    pub credentials: Credentials,
    pub upload: Option<UploadSource<'a>>,
    /// `-o/--output`: where to put the response payload.
    pub output: Option<OutputTarget>,
    pub pagination: PaginationConfig,
    pub sanitize: SanitizeConfig,
    pub format: OutputFormat,
    /// Return output values instead of printing them.
    pub capture_output: bool,
    pub options: ExecOptions,
}

/// Executes an API method call with the historical helper-facing signature.
///
/// Helpers call this; the CLI path uses [`execute`] through
/// [`options::run_from_matches`].
#[allow(clippy::too_many_arguments)]
pub async fn execute_method(
    doc: &RestDescription,
    method: &RestMethod,
    params_json: Option<&str>,
    body_json: Option<&str>,
    token: Option<&str>,
    auth_method: AuthMethod,
    output_path: Option<&str>,
    upload: Option<UploadSource<'_>>,
    dry_run: bool,
    pagination: &PaginationConfig,
    sanitize_template: Option<&str>,
    sanitize_mode: &SanitizeMode,
    output_format: &OutputFormat,
    capture_output: bool,
) -> Result<Option<Value>, GwsError> {
    let credentials = match (token, auth_method) {
        (Some(t), AuthMethod::OAuth) => Credentials::Static(t.to_string()),
        _ => Credentials::None,
    };
    let mut options = ExecOptions::from_env()?;
    options.dry_run = dry_run;
    execute(Invocation {
        doc,
        method,
        params: input::parse_params(params_json)?,
        body: input::parse_body(body_json)?,
        credentials,
        upload,
        output: output_path.map(|p| OutputTarget::File(PathBuf::from(p))),
        pagination: pagination.clone(),
        sanitize: SanitizeConfig {
            template: sanitize_template.map(str::to_string),
            mode: sanitize_mode.clone(),
        },
        format: output_format.clone(),
        capture_output,
        options,
    })
    .await
}

/// Executes an API method call, writing output to stdout.
pub async fn execute(inv: Invocation<'_>) -> Result<Option<Value>, GwsError> {
    execute_to(inv, &Emitter::stdout()).await
}

/// The planned upload for a call.
struct UploadPlan<'a> {
    data: UploadData<'a>,
    mime: String,
    resumable: bool,
}

async fn plan_upload<'a>(
    source: &UploadSource<'a>,
    method: &RestMethod,
    body: &Option<Value>,
    mode: UploadMode,
) -> Result<UploadPlan<'a>, GwsError> {
    let (data, mime) = match source {
        UploadSource::Bytes { data, content_type } => {
            if content_type.contains(['\r', '\n']) {
                return Err(GwsError::Validation(
                    "Upload content type must not contain CR or LF".to_string(),
                ));
            }
            (UploadData::Bytes(data), (*content_type).to_string())
        }
        UploadSource::File { path, content_type } => {
            let meta = tokio::fs::metadata(path).await.map_err(|e| {
                GwsError::Validation(format!("Failed to read upload file '{path}': {e}"))
            })?;
            if !meta.is_file() {
                return Err(GwsError::Validation(format!(
                    "Upload path '{path}' is not a regular file"
                )));
            }
            let mime = upload::resolve_upload_mime(*content_type, Some(path), body);
            (
                UploadData::File {
                    path: (*path).to_string(),
                    size: meta.len(),
                },
                mime,
            )
        }
    };
    let supports_resumable = url::resumable_upload_path(method).is_some();
    let resumable = match mode {
        UploadMode::Resumable if supports_resumable => true,
        UploadMode::Resumable => {
            return Err(GwsError::Validation(format!(
                "--upload-resumable: {} does not support resumable uploads",
                method.id.as_deref().unwrap_or("this method")
            )));
        }
        UploadMode::Auto => supports_resumable && data.len() > upload::RESUMABLE_THRESHOLD,
    };
    Ok(UploadPlan {
        data,
        mime,
        resumable,
    })
}

/// Apply `--fields` and Shared Drive defaults to the parameters.
fn apply_param_defaults(
    method: &RestMethod,
    params: &mut Map<String, Value>,
    fields: Option<&str>,
    paginating: bool,
) -> Result<(), GwsError> {
    if let Some(mask) = fields {
        if params.contains_key("fields") {
            return Err(GwsError::Validation(
                "--fields conflicts with \"fields\" in --params; use only one".to_string(),
            ));
        }
        // A mask that drops nextPageToken silently stops pagination.
        let mask = if paginating && !mask.contains("nextPageToken") {
            format!("nextPageToken,{mask}")
        } else {
            mask.to_string()
        };
        params.insert("fields".to_string(), Value::String(mask));
    }
    // Drive rejects items in shared drives unless supportsAllDrives is set.
    if method.parameters.contains_key("supportsAllDrives")
        && !params.contains_key("supportsAllDrives")
    {
        params.insert("supportsAllDrives".to_string(), Value::Bool(true));
    }
    Ok(())
}

/// POST/PATCH are retried only when they carry a `requestId` that the API
/// de-duplicates on (Calendar, Chat, Meet, Pub/Sub, ...).
fn idempotency(params: &Map<String, Value>, body: &Option<Value>) -> Idempotency {
    let has_request_id = params.contains_key("requestId")
        || body
            .as_ref()
            .and_then(|b| b.get("requestId"))
            .is_some_and(|v| !v.is_null());
    if has_request_id {
        Idempotency::Idempotent
    } else {
        Idempotency::FromMethod
    }
}

fn http_method(method: &RestMethod) -> Result<Method, GwsError> {
    Method::from_bytes(method.http_method.to_ascii_uppercase().as_bytes()).map_err(|_| {
        other(anyhow::anyhow!(
            "Unsupported HTTP method in Discovery document: {:?}",
            method.http_method
        ))
    })
}

struct Output<'a> {
    emitter: &'a Emitter,
    format: &'a OutputFormat,
    capture: bool,
    captured: Vec<Value>,
    lines_emitted: usize,
}

impl Output<'_> {
    fn single(&mut self, value: Value) -> Result<(), GwsError> {
        if self.capture {
            self.captured.push(value);
            return Ok(());
        }
        self.emitter.value(&value, self.format)
    }

    fn stream(&mut self, value: Value) -> Result<(), GwsError> {
        if self.capture {
            self.captured.push(value);
            return Ok(());
        }
        let first = self.lines_emitted == 0;
        self.lines_emitted += 1;
        self.emitter.page(&value, self.format, first)
    }

    fn finish(mut self) -> Option<Value> {
        match self.captured.len() {
            0 => None,
            1 => self.captured.pop(),
            _ => Some(Value::Array(self.captured)),
        }
    }
}

pub(crate) async fn execute_to(
    inv: Invocation<'_>,
    emitter: &Emitter,
) -> Result<Option<Value>, GwsError> {
    let Invocation {
        doc,
        method,
        mut params,
        mut body,
        credentials,
        upload,
        output,
        pagination,
        sanitize,
        format,
        capture_output,
        options,
    } = inv;
    let method_id = method.id.as_deref().unwrap_or("(unknown method)");
    let token_location = pagination::token_location(doc, method);
    let page_all = pagination.page_all || options.page_items.is_some();

    if page_all && token_location.is_none() {
        return Err(GwsError::Validation(format!(
            "--page-all: {method_id} does not paginate (it has no pageToken parameter)"
        )));
    }
    if page_all && output.is_some() {
        return Err(GwsError::Validation(
            "--output cannot be combined with --page-all; redirect stdout instead".to_string(),
        ));
    }
    if upload.is_some() && page_all {
        return Err(GwsError::Validation(
            "--upload cannot be combined with --page-all".to_string(),
        ));
    }

    apply_param_defaults(method, &mut params, options.fields.as_deref(), page_all)?;
    input::validate_params(doc, method, &params, options.allow_unknown_params)?;
    if let (Some(b), Some(schema)) = (
        &body,
        method
            .request
            .as_ref()
            .and_then(|r| r.schema_ref.as_deref()),
    ) {
        body_schema::validate_body(b, schema, doc, options.allow_unknown_fields)?;
    }

    let upload_plan = match &upload {
        Some(source) if method.supports_media_upload => {
            Some(plan_upload(source, method, &body, options.upload_mode).await?)
        }
        Some(_) => {
            return Err(GwsError::Validation(format!(
                "{method_id} does not accept media uploads"
            )));
        }
        None => None,
    };
    let target = match &upload_plan {
        Some(p) if p.resumable => UrlTarget::ResumableUpload,
        Some(_) => UrlTarget::SimpleUpload,
        None => UrlTarget::Method,
    };
    let request_url = build_url(doc, method, &params, target, &options.endpoints)?;
    let http = http_method(method)?;

    if options.dry_run {
        let info = json!({
            "dry_run": true,
            "url": request_url.url,
            "method": method.http_method,
            "query_params": request_url.query,
            "body": body,
            "upload": upload_plan.as_ref().map(|p| json!({
                "protocol": if p.resumable { "resumable" } else { "multipart" },
                "bytes": p.data.len(),
                "contentType": p.mime,
            })),
            "page_all": page_all,
        });
        if capture_output {
            return Ok(Some(info));
        }
        emitter.value(&info, &format)?;
        return Ok(None);
    }

    if safety::is_destructive(method) {
        safety::confirm(
            method_id,
            options.assume_yes,
            options.confirm,
            options.interactive,
            safety::ask_on_terminal,
        )?;
    }

    let quota_project = if options.quota_project {
        crate::auth::get_quota_project()
    } else {
        None
    };
    let transport = Transport::new(
        &credentials,
        options.retry.clone(),
        options.endpoints.clone(),
        quota_project,
    )?;
    let auth_method = credentials.auth_method();
    let idem = idempotency(&params, &body);
    let mut out = Output {
        emitter,
        format: &format,
        capture: capture_output,
        captured: Vec::new(),
        lines_emitted: 0,
    };
    let mut query = request_url.query.clone();
    let mut pages: u32 = 0;
    let mut item_field: Option<String> = None;

    loop {
        let sent = match (&upload_plan, pages) {
            (Some(plan), 0) => {
                send_upload(
                    &transport,
                    &http,
                    &request_url.url,
                    &query,
                    &body,
                    plan,
                    idem,
                    &options,
                )
                .await?
            }
            _ => send_plain(&transport, &http, &request_url.url, &query, &body, idem).await?,
        };
        let status = sent.response.status();
        let note = sent.retry_note();
        let content_type = sent
            .response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        tracing::debug!(
            api_method = method_id,
            status = status.as_u16(),
            attempts = sent.attempts,
            page = pages,
            "API response"
        );

        if !status.is_success() {
            let body_bytes = read_body(sent.response, options.idle_timeout()).await?;
            return Err(error_from_response(
                status,
                &String::from_utf8_lossy(&body_bytes),
                &auth_method,
                note.as_deref(),
            ));
        }
        // Empty success responses still produce a JSON document on stdout
        // (and never create an --output file).
        let empty_success = json!({"status": "success", "httpStatus": status.as_u16()});
        if status == reqwest::StatusCode::NO_CONTENT {
            out.single(empty_success)?;
            break;
        }

        let is_json =
            content_type.contains("application/json") || content_type.contains("text/json");
        if !is_json && !content_type.is_empty() {
            if let Some(summary) = download::deliver_non_json(
                sent.response,
                &content_type,
                output.as_ref(),
                options.idle_timeout(),
                emitter,
            )
            .await?
            {
                out.single(summary)?;
            }
            break;
        }

        let raw = read_body(sent.response, options.idle_timeout()).await?;
        if raw.iter().all(u8::is_ascii_whitespace) {
            out.single(empty_success)?;
            break;
        }
        let mut value: Value = match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(e) if is_json => {
                return Err(other(anyhow::anyhow!(
                    "{method_id} returned invalid JSON ({e}); first bytes: {}",
                    String::from_utf8_lossy(&raw[..raw.len().min(200)])
                )));
            }
            Err(_) => {
                // No content type and not JSON: save/stream it when asked,
                // otherwise wrap it as a JSON string (never raw on stdout).
                match &output {
                    Some(OutputTarget::File(path)) => {
                        download::write_file_atomic(path, &raw).await?;
                        out.single(json!({"status": "success", "saved_file": path.display().to_string(), "bytes": raw.len()}))?;
                    }
                    Some(OutputTarget::Stdout) => {
                        emitter.bytes(&raw).await?;
                        emitter.flush().await?;
                    }
                    None => out.single(download::wrap_text(status.as_u16(), "(none)", &raw)?)?,
                }
                break;
            }
        };
        pages += 1;

        if let Some(template) = &sanitize.template {
            value = sanitize::screen_with_model_armor(template, &sanitize.mode, value).await?;
        }

        // Long-running operations.
        let auto_wait = output.is_some() && operation::is_operation(&value);
        if operation::is_operation(&value) && (options.wait.is_some() || auto_wait) {
            let cfg = options.wait.clone().unwrap_or_default();
            let done = operation::wait(&transport, doc, value, &cfg).await?;
            value = operation::operation_result(done)?;
            if let Some(uri) = value.get("downloadUri").and_then(Value::as_str) {
                let uri = uri.to_string();
                let dl = transport
                    .send(Method::GET, &uri, Idempotency::Idempotent, |rb| rb)
                    .await?;
                let dl_status = dl.response.status();
                if !dl_status.is_success() {
                    let note = dl.retry_note();
                    let b = read_body(dl.response, options.idle_timeout()).await?;
                    return Err(error_from_response(
                        dl_status,
                        &String::from_utf8_lossy(&b),
                        &auth_method,
                        note.as_deref(),
                    ));
                }
                let ct = dl
                    .response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                if let Some(summary) = download::deliver_non_json(
                    dl.response,
                    &ct,
                    output.as_ref(),
                    options.idle_timeout(),
                    emitter,
                )
                .await?
                {
                    out.single(summary)?;
                }
                break;
            }
        }

        match &output {
            Some(OutputTarget::File(path)) => {
                let summary = download::save_json_response(
                    doc,
                    method,
                    &value,
                    path,
                    options.decode_field.as_deref(),
                )
                .await?;
                out.single(summary)?;
                break;
            }
            Some(OutputTarget::Stdout) => {
                // `-o -` with a base64 payload streams the decoded bytes;
                // otherwise the JSON is printed as usual.
                if let Some((_, bytes)) =
                    download::decoded_payload(doc, method, &value, options.decode_field.as_deref())?
                {
                    emitter.bytes(&bytes).await?;
                    emitter.flush().await?;
                    break;
                }
            }
            None => {}
        }

        let next = pagination::next_token(&value).map(str::to_string);
        if !page_all {
            out.single(value)?;
            break;
        }
        match &options.page_items {
            Some(mode) => {
                if item_field.is_none() {
                    let requested = match mode {
                        ItemsMode::Field(f) => Some(f.as_str()),
                        ItemsMode::Auto => None,
                    };
                    item_field = Some(pagination::item_field(doc, method, &value, requested)?);
                }
                let field = item_field.as_deref().unwrap_or_default();
                match value.get_mut(field).map(Value::take) {
                    Some(Value::Array(items)) => {
                        for item in items {
                            out.stream(item)?;
                        }
                    }
                    Some(Value::Null) | None => {}
                    Some(other_value) => {
                        return Err(GwsError::Validation(format!(
                            "--page-items: field '{field}' is {}, not an array",
                            body_schema::value_type(&other_value)
                        )));
                    }
                }
            }
            None => out.stream(value)?,
        }

        let Some(token) = next else { break };
        if pagination.page_limit != 0 && pages >= pagination.page_limit {
            eprintln!(
                "warning: stopped after {pages} page(s) because of --page-limit {}; more results are available \
                 (nextPageToken present). Use --page-limit 0 to fetch everything.",
                pagination.page_limit
            );
            if format == OutputFormat::Json {
                out.stream(json!({"_truncated": {"pagesFetched": pages, "nextPageToken": token}}))?;
            }
            break;
        }
        if let Some(loc) = token_location {
            pagination::place_token(loc, &token, &mut query, &mut body)?;
        }
        if pagination.page_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(pagination.page_delay_ms)).await;
        }
    }

    Ok(out.finish())
}

async fn send_plain(
    transport: &Transport,
    http: &Method,
    url: &str,
    query: &[(String, String)],
    body: &Option<Value>,
    idem: Idempotency,
) -> Result<Sent, GwsError> {
    let needs_length = matches!(*http, Method::POST | Method::PUT | Method::PATCH);
    transport
        .send(http.clone(), url, idem, |rb| {
            let rb = if query.is_empty() {
                rb
            } else {
                rb.query(query)
            };
            match body {
                Some(b) => rb.json(b),
                None if needs_length => rb.header(reqwest::header::CONTENT_LENGTH, 0),
                None => rb,
            }
        })
        .await
}

#[allow(clippy::too_many_arguments)]
async fn send_upload(
    transport: &Transport,
    http: &Method,
    url: &str,
    query: &[(String, String)],
    body: &Option<Value>,
    plan: &UploadPlan<'_>,
    idem: Idempotency,
    options: &ExecOptions,
) -> Result<Sent, GwsError> {
    let mut query = query.to_vec();
    if plan.resumable {
        query.push(("uploadType".to_string(), "resumable".to_string()));
        upload::resumable_upload(
            transport,
            upload::Resumable {
                method: http.clone(),
                url,
                query: &query,
                metadata: body,
                data: &plan.data,
                media_mime: &plan.mime,
                chunk_size: options.upload_chunk_size,
            },
        )
        .await
    } else {
        query.push(("uploadType".to_string(), "multipart".to_string()));
        upload::send_multipart(
            transport,
            http.clone(),
            url,
            &query,
            body,
            &plan.data,
            &plan.mime,
            idem,
        )
        .await
    }
}
