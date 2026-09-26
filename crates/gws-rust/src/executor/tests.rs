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

//! Wiremock-based integration tests for the executor: pagination, retries,
//! token refresh, endpoint validation, uploads, downloads, long-running
//! operations, the destructive gate and batch requests.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Map, Value, json};
use wiremock::matchers::{body_json, header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use gws_rust_core::client::RetryPolicy;
use gws_rust_core::validate::EndpointPolicy;

use super::output::Emitter;
use super::*;
use crate::discovery::{
    JsonSchema, JsonSchemaProperty, MediaUpload, MediaUploadProtocol, MediaUploadProtocols,
    MethodParameter, RestResource, SchemaRef,
};

// ── Fixtures ────────────────────────────────────────────────────────────

fn qparam(ty: &str) -> MethodParameter {
    MethodParameter {
        param_type: Some(ty.into()),
        location: Some("query".into()),
        ..Default::default()
    }
}

fn pparam() -> MethodParameter {
    MethodParameter {
        param_type: Some("string".into()),
        location: Some("path".into()),
        required: true,
        ..Default::default()
    }
}

fn sref(name: &str) -> Option<SchemaRef> {
    Some(SchemaRef {
        schema_ref: Some(name.into()),
        parameter_name: None,
    })
}

fn prop(ty: &str) -> JsonSchemaProperty {
    JsonSchemaProperty {
        prop_type: Some(ty.into()),
        ..Default::default()
    }
}

/// A small Discovery document rooted at the mock server.
fn doc(root: &str) -> RestDescription {
    let mut list_params = HashMap::new();
    list_params.insert("pageToken".into(), qparam("string"));
    list_params.insert("pageSize".into(), qparam("integer"));
    let mut id_param = HashMap::new();
    id_param.insert("fileId".into(), pparam());
    let mut create_params = HashMap::new();
    create_params.insert("uploadType".into(), qparam("string"));
    create_params.insert("supportsAllDrives".into(), qparam("boolean"));

    let mut files = RestResource::default();
    files.methods.insert(
        "list".into(),
        RestMethod {
            id: Some("drive.files.list".into()),
            http_method: "GET".into(),
            path: "files".into(),
            parameters: list_params,
            response: sref("FileList"),
            ..Default::default()
        },
    );
    files.methods.insert(
        "get".into(),
        RestMethod {
            id: Some("drive.files.get".into()),
            http_method: "GET".into(),
            path: "files/{fileId}".into(),
            parameters: id_param.clone(),
            supports_media_download: true,
            ..Default::default()
        },
    );
    files.methods.insert(
        "delete".into(),
        RestMethod {
            id: Some("drive.files.delete".into()),
            http_method: "DELETE".into(),
            path: "files/{fileId}".into(),
            parameters: id_param.clone(),
            ..Default::default()
        },
    );
    files.methods.insert(
        "download".into(),
        RestMethod {
            id: Some("drive.files.download".into()),
            http_method: "POST".into(),
            path: "files/{fileId}/download".into(),
            parameters: id_param.clone(),
            response: sref("Operation"),
            ..Default::default()
        },
    );
    files.methods.insert(
        "create".into(),
        RestMethod {
            id: Some("drive.files.create".into()),
            http_method: "POST".into(),
            path: "files".into(),
            parameters: create_params,
            request: sref("File"),
            response: sref("File"),
            supports_media_upload: true,
            media_upload: Some(MediaUpload {
                protocols: Some(MediaUploadProtocols {
                    simple: Some(MediaUploadProtocol {
                        path: "/upload/drive/v3/files".into(),
                        multipart: Some(true),
                    }),
                    resumable: Some(MediaUploadProtocol {
                        path: "/resumable/upload/drive/v3/files".into(),
                        multipart: Some(true),
                    }),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    files.methods.insert(
        "query".into(),
        RestMethod {
            id: Some("drive.files.query".into()),
            http_method: "POST".into(),
            path: "files:query".into(),
            request: sref("QueryRequest"),
            response: sref("FileList"),
            ..Default::default()
        },
    );

    let mut att_params = HashMap::new();
    att_params.insert("id".into(), pparam());
    files.methods.insert(
        "attachment".into(),
        RestMethod {
            id: Some("drive.files.attachment".into()),
            http_method: "GET".into(),
            path: "attachments/{id}".into(),
            parameters: att_params,
            response: sref("MessagePartBody"),
            ..Default::default()
        },
    );

    let mut ops = RestResource::default();
    let mut name_param = HashMap::new();
    name_param.insert("name".into(), pparam());
    ops.methods.insert(
        "get".into(),
        RestMethod {
            id: Some("drive.operations.get".into()),
            http_method: "GET".into(),
            path: "operations/{name}".into(),
            parameters: name_param,
            parameter_order: vec!["name".into()],
            response: sref("Operation"),
            ..Default::default()
        },
    );

    let mut schemas = HashMap::new();
    schemas.insert(
        "FileList".into(),
        JsonSchema {
            properties: [("files", prop("array")), ("nextPageToken", prop("string"))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            ..Default::default()
        },
    );
    schemas.insert(
        "File".into(),
        JsonSchema {
            schema_type: Some("object".into()),
            properties: [
                ("name", prop("string")),
                ("mimeType", prop("string")),
                ("id", prop("string")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            ..Default::default()
        },
    );
    schemas.insert(
        "QueryRequest".into(),
        JsonSchema {
            schema_type: Some("object".into()),
            properties: [("filter", prop("string")), ("pageToken", prop("string"))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            ..Default::default()
        },
    );
    schemas.insert("Operation".into(), JsonSchema::default());
    schemas.insert(
        "MessagePartBody".into(),
        JsonSchema {
            properties: [
                (
                    "data",
                    JsonSchemaProperty {
                        format: Some("byte".into()),
                        ..prop("string")
                    },
                ),
                ("size", prop("integer")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            ..Default::default()
        },
    );

    let mut d = RestDescription {
        name: "drive".into(),
        version: "v3".into(),
        root_url: format!("{root}/"),
        service_path: "drive/v3/".into(),
        schemas,
        ..Default::default()
    };
    d.parameters.insert("fields".into(), qparam("string"));
    d.resources.insert("files".into(), files);
    d.resources.insert("operations".into(), ops);
    d
}

fn options(server: &MockServer) -> ExecOptions {
    ExecOptions {
        dry_run: false,
        fields: None,
        page_items: None,
        wait: None,
        retry: RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            max_retry_after: Duration::from_secs(1),
            response_timeout: Some(Duration::from_secs(5)),
        },
        endpoints: EndpointPolicy::with_override(&server.uri()).unwrap(),
        quota_project: false,
        upload_mode: UploadMode::Auto,
        upload_chunk_size: upload::CHUNK_GRANULARITY,
        decode_field: None,
        allow_unknown_params: false,
        allow_unknown_fields: false,
        assume_yes: false,
        interactive: false,
    }
}

fn rest_method<'a>(d: &'a RestDescription, name: &str) -> &'a RestMethod {
    if let Some(m) = d.resources["files"].methods.get(name) {
        return m;
    }
    &d.resources["operations"].methods[name]
}

fn params(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

struct Call<'a> {
    doc: &'a RestDescription,
    method: &'a str,
    params: Value,
    body: Option<Value>,
    credentials: Credentials,
    upload: Option<UploadSource<'a>>,
    output: Option<OutputTarget>,
    pagination: PaginationConfig,
    options: ExecOptions,
    format: OutputFormat,
}

impl<'a> Call<'a> {
    fn new(doc: &'a RestDescription, method: &'a str, server: &MockServer) -> Self {
        Self {
            doc,
            method,
            params: json!({}),
            body: None,
            credentials: Credentials::Static("tok".into()),
            upload: None,
            output: None,
            pagination: PaginationConfig::default(),
            options: options(server),
            format: OutputFormat::Json,
        }
    }

    async fn run(self, emitter: &Emitter) -> Result<Option<Value>, GwsError> {
        execute_to(
            Invocation {
                doc: self.doc,
                method: rest_method(self.doc, self.method),
                params: params(self.params),
                body: self.body,
                credentials: self.credentials,
                upload: self.upload,
                output: self.output,
                pagination: self.pagination,
                sanitize: SanitizeConfig::default(),
                format: self.format,
                capture_output: false,
                options: self.options,
            },
            emitter,
        )
        .await
    }
}

fn lines(emitter: &Emitter) -> Vec<Value> {
    // Pages are NDJSON; single values are pretty-printed across lines.
    serde_json::Deserializer::from_str(&emitter.captured_text())
        .into_iter::<Value>()
        .map(|v| v.unwrap())
        .collect()
}

/// Mounts three list pages chained by page tokens.
async fn mount_pages(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"files": [{"id": "1"}, {"id": "2"}], "nextPageToken": "p2"})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("pageToken", "p2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"files": [{"id": "3"}], "nextPageToken": "p3"})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("pageToken", "p3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [{"id": "4"}]})))
        .mount(server)
        .await;
}

struct Seq {
    responses: Vec<ResponseTemplate>,
    calls: Arc<AtomicU32>,
}

impl Respond for Seq {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
        self.responses[n.min(self.responses.len() - 1)].clone()
    }
}

fn seq(responses: Vec<ResponseTemplate>) -> (Seq, Arc<AtomicU32>) {
    let calls = Arc::new(AtomicU32::new(0));
    (
        Seq {
            responses,
            calls: calls.clone(),
        },
        calls,
    )
}

// ── Pagination ──────────────────────────────────────────────────────────

#[tokio::test]
async fn page_all_is_unlimited_by_default() {
    let server = MockServer::start().await;
    mount_pages(&server).await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination = PaginationConfig {
        page_all: true,
        page_limit: 0,
        page_delay_ms: 0,
    };
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let out = lines(&em);
    assert_eq!(out.len(), 3);
    assert_eq!(out[2]["files"][0]["id"], "4");
}

/// Mounts two list pages whose items have different key sets and order.
async fn mount_ragged_pages(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"files": [{"id": "1", "name": "a"}], "nextPageToken": "p2"})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
            {"extra": "x", "name": "b", "id": "2"},
            {"id": "3"}
        ]})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn page_all_csv_rows_align_with_the_first_page_header() {
    let server = MockServer::start().await;
    mount_ragged_pages(&server).await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination.page_all = true;
    call.pagination.page_delay_ms = 0;
    call.format = OutputFormat::Csv;
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(em.captured_text(), "id,name\n1,a\n2,b\n3,\n");
}

#[tokio::test]
async fn page_items_table_rows_align_with_the_first_item_header() {
    let server = MockServer::start().await;
    mount_ragged_pages(&server).await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination.page_delay_ms = 0;
    call.options.page_items = Some(ItemsMode::Auto);
    call.format = OutputFormat::Table;
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(em.captured_text(), "id  name\n──  ────\n1   a\n2   b\n3\n");
}

#[tokio::test]
async fn page_limit_truncation_is_marked() {
    let server = MockServer::start().await;
    mount_pages(&server).await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination = PaginationConfig {
        page_all: true,
        page_limit: 2,
        page_delay_ms: 0,
    };
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let out = lines(&em);
    assert_eq!(out.len(), 3, "two pages plus the truncation marker");
    assert_eq!(
        out[2],
        json!({"_truncated": {"pagesFetched": 2, "nextPageToken": "p3"}})
    );
}

#[tokio::test]
async fn page_all_repeated_token_is_an_error_not_an_infinite_loop() {
    let server = MockServer::start().await;
    // A misbehaving API that hands back the same page token forever.
    let (responder, calls) = seq(vec![
        ResponseTemplate::new(200)
            .set_body_json(json!({"files": [{"id": "1"}], "nextPageToken": "p2"})),
        ResponseTemplate::new(200)
            .set_body_json(json!({"files": [{"id": "2"}], "nextPageToken": "p2"})),
    ]);
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .respond_with(responder)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination = PaginationConfig {
        page_all: true,
        page_limit: 0,
        page_delay_ms: 0,
    };
    let em = Emitter::capturing();
    let result = tokio::time::timeout(Duration::from_secs(5), call.run(&em)).await;
    let n = calls.load(Ordering::SeqCst);
    let Ok(result) = result else {
        panic!("--page-all did not terminate: {n} requests sent for a repeated nextPageToken");
    };
    let err = result.unwrap_err();
    assert!(err.to_string().contains("repeated nextPageToken"), "{err}");
    assert_eq!(n, 2, "stops as soon as the token repeats");
    // The pages that were fetched before the repeat were still emitted.
    assert_eq!(lines(&em).len(), 2);
}

#[tokio::test]
async fn page_items_emits_one_line_per_item() {
    let server = MockServer::start().await;
    mount_pages(&server).await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.pagination.page_delay_ms = 0;
    call.options.page_items = Some(ItemsMode::Auto);
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let ids: Vec<String> = lines(&em)
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["1", "2", "3", "4"]);
}

#[tokio::test]
async fn body_page_token_is_injected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files:query"))
        .and(body_json(json!({"filter": "x"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"files": [1], "nextPageToken": "b2"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files:query"))
        .and(body_json(json!({"filter": "x", "pageToken": "b2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [2]})))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "query", &server);
    call.body = Some(json!({"filter": "x"}));
    call.pagination = PaginationConfig {
        page_all: true,
        page_limit: 0,
        page_delay_ms: 0,
    };
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(lines(&em).len(), 2);
}

#[tokio::test]
async fn page_all_rejected_for_non_paginating_method() {
    let server = MockServer::start().await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    call.pagination.page_all = true;
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("does not paginate"));
}

#[tokio::test]
async fn fields_flag_keeps_next_page_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("fields", "nextPageToken,files(id)"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": []})))
        .expect(1)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.options.fields = Some("files(id)".into());
    call.pagination.page_all = true;
    call.run(&Emitter::capturing()).await.unwrap();
}

// ── Validation ──────────────────────────────────────────────────────────

#[tokio::test]
async fn unknown_param_is_rejected_before_sending() {
    let server = MockServer::start().await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.params = json!({"pageSze": 5});
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("did you mean 'pageSize'"), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn dry_run_sends_nothing_and_reports_request() {
    let server = MockServer::start().await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "delete", &server);
    call.params = json!({"fileId": "abc"});
    call.options.dry_run = true;
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let out = lines(&em);
    assert_eq!(out[0]["dry_run"], true);
    assert!(
        out[0]["url"]
            .as_str()
            .unwrap()
            .ends_with("/drive/v3/files/abc")
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

// ── Retry and auth ──────────────────────────────────────────────────────

#[tokio::test]
async fn get_is_retried_on_503() {
    let server = MockServer::start().await;
    let (responder, calls) = seq(vec![
        ResponseTemplate::new(503),
        ResponseTemplate::new(200).set_body_json(json!({"id": "1"})),
    ]);
    Mock::given(method("GET"))
        .and(path("/drive/v3/files/1"))
        .respond_with(responder)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(lines(&em)[0]["id"], "1");
}

#[tokio::test]
async fn exhausted_retries_surface_real_error_with_note() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files/1"))
        .respond_with(ResponseTemplate::new(503).set_body_json(
            json!({"error": {"code": 503, "message": "Backend Error", "errors": [{"reason": "backendError"}]}}),
        ))
        .expect(3)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    match err {
        GwsError::Api {
            code,
            message,
            reason,
            ..
        } => {
            assert_eq!(code, 503);
            assert_eq!(reason, "backendError");
            assert!(message.contains("gave up after 3 attempts"), "{message}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn post_is_never_retried_on_5xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files:query"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "query", &server);
    call.body = Some(json!({"filter": "x"}));
    assert!(call.run(&Emitter::capturing()).await.is_err());
}

struct QueueProvider(tokio::sync::Mutex<Vec<&'static str>>);

#[async_trait::async_trait]
impl crate::auth::AccessTokenProvider for QueueProvider {
    async fn refresh_access_token(&self) -> anyhow::Result<String> {
        self.0
            .lock()
            .await
            .pop()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("no token"))
    }
}

#[tokio::test]
async fn refreshes_token_on_401_during_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(header("authorization", "Bearer old"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"files": [1], "nextPageToken": "p2"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(header("authorization", "Bearer old"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(header("authorization", "Bearer new"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [2]})))
        .expect(1)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "list", &server);
    call.credentials = Credentials::Refreshable {
        token: "old".into(),
        provider: Arc::new(QueueProvider(tokio::sync::Mutex::new(vec!["new"]))),
    };
    call.pagination = PaginationConfig {
        page_all: true,
        page_limit: 0,
        page_delay_ms: 0,
    };
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(lines(&em).len(), 2);
}

#[tokio::test]
async fn static_token_401_is_reported_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            json!({"error": {"code": 401, "message": "Invalid Credentials", "errors": [{"reason": "authError"}]}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("Invalid Credentials"));
}

// ── Endpoint policy (SEC-04) ────────────────────────────────────────────

#[tokio::test]
async fn token_is_never_sent_to_untrusted_root() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    // A tampered Discovery doc points rootUrl at the mock server, but no
    // override is configured, so only *.googleapis.com is trusted.
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    call.options.endpoints = EndpointPolicy::google_only();
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("Refusing to"), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn http_googleapis_root_is_rejected() {
    let server = MockServer::start().await;
    let d = doc("http://www.googleapis.com");
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    call.options.endpoints = EndpointPolicy::google_only();
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("Refusing to"), "{err}");
}

// ── Destructive gate ────────────────────────────────────────────────────

#[tokio::test]
async fn destructive_call_requires_yes_without_a_terminal() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "delete", &server);
    call.params = json!({"fileId": "1"});
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(matches!(err, GwsError::ConfirmationRequired(_)), "{err:?}");
    assert!(err.to_string().contains("--yes"));
    assert!(server.received_requests().await.unwrap().is_empty());

    let mut call = Call::new(&d, "delete", &server);
    call.params = json!({"fileId": "1"});
    call.options.assume_yes = true;
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(
        lines(&em),
        vec![json!({"status": "success", "httpStatus": 204})],
        "204 prints a JSON status and writes no file"
    );
}

// ── Uploads ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn small_file_uses_multipart() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/drive/v3/files"))
        .and(query_param("uploadType", "multipart"))
        .and(query_param("supportsAllDrives", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "new"})))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"hello").unwrap();
    let path = file.to_str().unwrap().to_string();
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "create", &server);
    call.body = Some(json!({"name": "a.txt"}));
    call.upload = Some(UploadSource {
        path: &path,
        content_type: None,
    });
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let received = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&received[0].body);
    assert!(body.contains("Content-Type: text/plain"));
    assert!(body.contains("hello"));
    assert_eq!(lines(&em)[0]["id"], "new");
}

#[tokio::test]
async fn resumable_upload_resumes_after_chunk_failure() {
    let server = MockServer::start().await;
    let chunk = upload::CHUNK_GRANULARITY as u64;
    let total = chunk * 2 + 1000;
    let session = format!("{}/session/xyz", server.uri());

    Mock::given(method("POST"))
        .and(path("/resumable/upload/drive/v3/files"))
        .and(query_param("uploadType", "resumable"))
        .and(header(
            "x-upload-content-length",
            total.to_string().as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).insert_header("location", session.as_str()))
        .expect(1)
        .mount(&server)
        .await;
    // Chunk 1 accepted.
    Mock::given(method("PUT"))
        .and(path("/session/xyz"))
        .and(header(
            "content-range",
            format!("bytes 0-{}/{total}", chunk - 1).as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(308)
                .insert_header("range", format!("bytes=0-{}", chunk - 1).as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    // Chunk 2 fails once with 503, then succeeds.
    let (chunk2, chunk2_calls) = seq(vec![
        ResponseTemplate::new(503),
        ResponseTemplate::new(308)
            .insert_header("range", format!("bytes=0-{}", 2 * chunk - 1).as_str()),
    ]);
    Mock::given(method("PUT"))
        .and(path("/session/xyz"))
        .and(header(
            "content-range",
            format!("bytes {chunk}-{}/{total}", 2 * chunk - 1).as_str(),
        ))
        .respond_with(chunk2)
        .mount(&server)
        .await;
    // Status query after the failure: server has chunk 1 only.
    Mock::given(method("PUT"))
        .and(path("/session/xyz"))
        .and(header("content-range", format!("bytes */{total}").as_str()))
        .respond_with(
            ResponseTemplate::new(308)
                .insert_header("range", format!("bytes=0-{}", chunk - 1).as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    // Final chunk completes the upload.
    Mock::given(method("PUT"))
        .and(path("/session/xyz"))
        .and(header(
            "content-range",
            format!("bytes {}-{}/{total}", 2 * chunk, total - 1).as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "big"})))
        .expect(1)
        .mount(&server)
        .await;

    let data: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("big.bin");
    std::fs::write(&file, &data).unwrap();
    let file_path = file.to_str().unwrap().to_string();
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "create", &server);
    call.body = Some(json!({"name": "big.bin"}));
    call.upload = Some(UploadSource {
        path: &file_path,
        content_type: Some("application/octet-stream"),
    });
    call.options.upload_mode = UploadMode::Resumable;
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(chunk2_calls.load(Ordering::SeqCst), 2);
    assert_eq!(lines(&em)[0]["id"], "big");

    // The bytes the server received for the final chunk are the tail of the data.
    let received = server.received_requests().await.unwrap();
    let last = received.last().unwrap();
    assert_eq!(&last.body[..], &data[(2 * chunk) as usize..]);
}

#[tokio::test]
async fn upload_session_uri_on_foreign_host_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("location", "https://evil.example/session"),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.txt");
    std::fs::write(&file, b"x").unwrap();
    let file_path = file.to_str().unwrap().to_string();
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "create", &server);
    call.upload = Some(UploadSource {
        path: &file_path,
        content_type: None,
    });
    call.options.upload_mode = UploadMode::Resumable;
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(
        err.to_string().contains("does not match request origin"),
        "{err}"
    );
}

// ── Downloads ───────────────────────────────────────────────────────────

#[tokio::test]
async fn binary_download_streams_to_stdout_only_with_dash_output() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files/1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(b"%PDF-bytes".to_vec(), "application/pdf"),
        )
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    call.output = Some(OutputTarget::Stdout);
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(em.captured_bytes(), b"%PDF-bytes");
}

#[tokio::test]
async fn binary_download_without_output_is_a_json_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"x".to_vec(), "image/png"))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let em = Emitter::capturing();
    let err = call.run(&em).await.unwrap_err();
    assert!(err.to_string().contains("-o PATH"), "{err}");
    assert!(em.captured_bytes().is_empty(), "nothing raw on stdout");
}

#[tokio::test]
async fn json_numbers_pass_through_unchanged() {
    // Doubles that serde_json's default (non-roundtrip) float parser reads
    // one ULP off, e.g. coordinates or unformatted Sheets values.
    let numbers = [
        "13.346133595589677",
        "-13.336664635114445",
        "10.938711676632721",
        "1.0715660391465826e-75",
    ];
    let body = format!("{{\"values\":[{}]}}", numbers.join(","));
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let printed: Vec<String> = lines(&em)[0]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.to_string())
        .collect();
    assert_eq!(printed, numbers, "{}", em.captured_text());
}

#[tokio::test]
async fn text_response_is_wrapped_in_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"a,b\n1,2\n".to_vec(), "text/csv"))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    let out = lines(&em);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["contentType"], "text/csv");
    assert_eq!(out[0]["body"], "a,b\n1,2\n");
    assert_eq!(out[0]["bytes"], 8);
}

#[tokio::test]
async fn binary_download_to_file_is_atomic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"content".to_vec(), "image/png"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("img.png");
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "get", &server);
    call.params = json!({"fileId": "1"});
    call.output = Some(OutputTarget::File(out.clone()));
    let em = Emitter::capturing();
    call.run(&em).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"content");
    assert_eq!(lines(&em)[0]["bytes"], 7);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no temp files left"
    );
}

#[tokio::test]
async fn json_output_decodes_base64_attachment() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/attachments/a1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": "aGk_", "size": 3})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("att.bin");
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "attachment", &server);
    call.params = json!({"id": "a1"});
    call.output = Some(OutputTarget::File(out.clone()));
    call.run(&Emitter::capturing()).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"hi?");
}

// ── Long-running operations ─────────────────────────────────────────────

#[tokio::test]
async fn download_operation_is_waited_on_and_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files/f1/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"name": "op1", "done": false, "kind": "drive#operation"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let download_uri = format!("{}/download/f1", server.uri());
    Mock::given(method("GET"))
        .and(path("/drive/v3/operations/op1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "op1", "done": true,
            "response": {"downloadUri": download_uri}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/f1"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"exported".to_vec(), "text/plain"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("f1.txt");
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "download", &server);
    call.params = json!({"fileId": "f1"});
    call.output = Some(OutputTarget::File(out.clone()));
    call.options.wait = Some(WaitConfig {
        timeout: Duration::from_secs(5),
        initial_interval: Duration::from_millis(1),
        max_interval: Duration::from_millis(5),
    });
    call.run(&Emitter::capturing()).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"exported");
}

#[tokio::test]
async fn failed_operation_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"name": "op2", "done": false})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/operations/op2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "op2", "done": true, "error": {"code": 13, "message": "export failed"}
        })))
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let mut call = Call::new(&d, "download", &server);
    call.params = json!({"fileId": "f1"});
    call.options.wait = Some(WaitConfig {
        timeout: Duration::from_secs(5),
        initial_interval: Duration::from_millis(1),
        max_interval: Duration::from_millis(5),
    });
    let err = call.run(&Emitter::capturing()).await.unwrap_err();
    assert!(err.to_string().contains("export failed"));
}

// ── Batch ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_round_trip_reports_partial_failure() {
    let server = MockServer::start().await;
    let response_body = "--b\r\nContent-Type: application/http\r\nContent-ID: <response-item-a>\r\n\r\n\
                         HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"id\":\"1\"}\r\n\
                         --b\r\nContent-Type: application/http\r\nContent-ID: <response-item-2>\r\n\r\n\
                         HTTP/1.1 404 Not Found\r\n\r\n{\"error\":{\"code\":404}}\r\n--b--\r\n";
    Mock::given(method("POST"))
        .and(path("/batch/drive/v3"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(response_body, "multipart/mixed; boundary=b"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let opts = options(&server);
    let calls = batch::parse_calls(
        &d,
        "{\"id\":\"a\",\"method\":\"files.get\",\"params\":{\"fileId\":\"1\"}}\n{\"method\":\"files.get\",\"params\":{\"fileId\":\"2\"}}",
        &opts,
    )
    .unwrap();
    let em = Emitter::capturing();
    let err = batch::run_batch(
        &d,
        &calls,
        Credentials::Static("tok".into()),
        &opts,
        &SanitizeConfig::default(),
        &em,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("1 of 2 batch call(s) failed"));
    let out = lines(&em);
    assert_eq!(
        out[0],
        json!({"id": "a", "status": 200, "body": {"id": "1"}})
    );
    assert_eq!(out[1]["status"], 404);
    let sent = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&sent[0].body);
    assert!(body.contains("GET /drive/v3/files/1 HTTP/1.1"));
}

#[tokio::test]
async fn batch_results_follow_input_order_when_parts_are_reordered() {
    // Parts are matched by Content-ID; Google does not promise to return them
    // in request order, but the output contract is one line per call in input
    // order.
    let server = MockServer::start().await;
    let response_body = "--b\r\nContent-Type: application/http\r\nContent-ID: <response-item-b>\r\n\r\n\
                         HTTP/1.1 404 Not Found\r\n\r\n{\"error\":{\"code\":404}}\r\n\
                         --b\r\nContent-Type: application/http\r\nContent-ID: <response-item-a>\r\n\r\n\
                         HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"id\":\"1\"}\r\n--b--\r\n";
    Mock::given(method("POST"))
        .and(path("/batch/drive/v3"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(response_body, "multipart/mixed; boundary=b"),
        )
        .mount(&server)
        .await;
    let d = doc(&server.uri());
    let opts = options(&server);
    let calls = batch::parse_calls(
        &d,
        "{\"id\":\"a\",\"method\":\"files.get\",\"params\":{\"fileId\":\"1\"}}\n{\"id\":\"b\",\"method\":\"files.get\",\"params\":{\"fileId\":\"2\"}}",
        &opts,
    )
    .unwrap();
    let em = Emitter::capturing();
    batch::run_batch(
        &d,
        &calls,
        Credentials::Static("tok".into()),
        &opts,
        &SanitizeConfig::default(),
        &em,
    )
    .await
    .unwrap_err();
    let ids: Vec<Value> = lines(&em).iter().map(|l| l["id"].clone()).collect();
    assert_eq!(ids, vec![json!("a"), json!("b")]);
}

#[test]
#[serial_test::serial]
fn batch_ids_must_be_unique() {
    // The second line's generated id ("2") collides with the first line's
    // explicit one: both results would be reported as id "2".
    let d = doc("https://www.googleapis.com");
    let mut opts = ExecOptions::from_env().unwrap();
    opts.endpoints = EndpointPolicy::google_only();
    let result = batch::parse_calls(
        &d,
        "{\"id\":\"2\",\"method\":\"files.get\",\"params\":{\"fileId\":\"1\"}}\n{\"method\":\"files.get\",\"params\":{\"fileId\":\"2\"}}",
        &opts,
    )
    .map(|calls| calls.iter().map(|c| c.id.clone()).collect::<Vec<_>>());
    let err = match result {
        Ok(ids) => panic!("duplicate ids were accepted: {ids:?}"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("duplicate id \"2\""), "{err}");
}
