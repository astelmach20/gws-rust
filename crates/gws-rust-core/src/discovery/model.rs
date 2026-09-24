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

//! Discovery Document model types and accessors.

use std::collections::HashMap;

use reqwest::Url;
use serde::Deserialize;

use crate::error::GwsError;
use crate::validate::{is_dangerous_unicode, validate_api_base_with};

/// Top-level Discovery REST Description document.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestDescription {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub root_url: String,
    /// mTLS variant of `root_url` (e.g. `https://drive.mtls.googleapis.com/`).
    pub mtls_root_url: Option<String>,
    #[serde(default)]
    pub service_path: String,
    pub base_url: Option<String>,
    /// Batch endpoint path relative to `root_url` (e.g. `batch/drive/v3`).
    pub batch_path: Option<String>,
    #[serde(default)]
    pub schemas: HashMap<String, JsonSchema>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    pub auth: Option<AuthDescription>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthDescription {
    pub oauth2: Option<OAuth2Description>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OAuth2Description {
    pub scopes: Option<HashMap<String, ScopeDescription>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScopeDescription {
    pub description: Option<String>,
}

/// A resource in the Discovery Document, which can contain methods and nested sub-resources.
#[derive(Debug, Deserialize, Default)]
pub struct RestResource {
    #[serde(default)]
    pub methods: HashMap<String, RestMethod>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
}

/// A single API method.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestMethod {
    pub id: Option<String>,
    pub description: Option<String>,
    pub http_method: String,
    pub path: String,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    #[serde(default)]
    pub parameter_order: Vec<String>,
    pub request: Option<SchemaRef>,
    pub response: Option<SchemaRef>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub flat_path: Option<String>,
    #[serde(default)]
    pub supports_media_download: bool,
    #[serde(default)]
    pub supports_media_upload: bool,
    pub media_upload: Option<MediaUpload>,
}

/// Media upload metadata from the Discovery Document.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaUpload {
    pub protocols: Option<MediaUploadProtocols>,
    pub accept: Option<Vec<String>>,
    /// Maximum upload size as written in the document (e.g. `5120GB`).
    /// Use [`RestMethod::max_upload_size_bytes`] for a parsed value.
    pub max_size: Option<String>,
}

/// Upload protocol details.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocols {
    pub simple: Option<MediaUploadProtocol>,
    /// Resumable upload protocol (`uploadType=resumable`), when supported.
    pub resumable: Option<MediaUploadProtocol>,
}

/// A single upload protocol entry.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocol {
    pub path: String,
    pub multipart: Option<bool>,
}

/// A reference to a schema (e.g., `{ "$ref": "File" }`).
#[derive(Debug, Deserialize, Default)]
pub struct SchemaRef {
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    #[serde(rename = "parameterName")]
    pub parameter_name: Option<String>,
}

/// A parameter definition for a method.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MethodParameter {
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub format: Option<String>,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub enum_descriptions: Option<Vec<String>>,
    #[serde(default)]
    pub repeated: bool,
    pub minimum: Option<String>,
    pub maximum: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
}

/// JSON Schema definition for request/response bodies.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchema {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub schema_type: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub required: Vec<String>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// A property within a JSON Schema.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaProperty {
    #[serde(rename = "type")]
    pub prop_type: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub format: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(default)]
    pub read_only: bool,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// Where a method's page token is sent on the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PageTokenLocation {
    /// As the `pageToken` query parameter.
    Query,
    /// As the `pageToken` field of the JSON request body.
    Body,
}

/// Pagination metadata for a method, derived from the Discovery Document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pagination {
    /// Where to send the token for the next page.
    pub token_location: PageTokenLocation,
    /// Request parameter / body field name carrying the token (`pageToken`).
    pub request_field: &'static str,
    /// Response field holding the next token (`nextPageToken`).
    pub response_field: &'static str,
}

const PAGE_TOKEN: &str = "pageToken";
const NEXT_PAGE_TOKEN: &str = "nextPageToken";

impl RestDescription {
    /// Look up a named schema.
    pub fn schema(&self, name: &str) -> Option<&JsonSchema> {
        self.schemas.get(name)
    }

    /// The API root that requests (and credentials) go to.
    ///
    /// Returns `override_base` when given (the validated `GWSR_API_BASE_URL`,
    /// see [`crate::validate::api_base_override`]); otherwise the document's
    /// `rootUrl`, which must pass [`validate_api_base_with`].
    pub fn api_root(&self, override_base: Option<&Url>) -> Result<Url, GwsError> {
        match override_base {
            Some(base) => Ok(base.clone()),
            None => validate_api_base_with(&self.root_url, None),
        }
    }

    /// The service base URL requests are built from: `baseUrl` when the
    /// document has one and no override is set, otherwise
    /// [`api_root`](Self::api_root) + `servicePath`. Method paths are appended
    /// to it verbatim.
    pub fn service_base(&self, override_base: Option<&Url>) -> Result<String, GwsError> {
        if override_base.is_none()
            && let Some(base) = &self.base_url
        {
            validate_api_base_with(base, None)?;
            return Ok(base.clone());
        }
        let root = self.api_root(override_base)?;
        let service_path = self.service_path.trim_start_matches('/');
        Ok(format!("{root}{service_path}"))
    }

    /// Validate every endpoint-bearing field before the document is trusted
    /// (SEC-04): `rootUrl`, `mtlsRootUrl`, `baseUrl` must be trusted API bases,
    /// and `servicePath`, `batchPath`, and every method/upload path must be
    /// relative paths that cannot change the request origin.
    pub fn validate_endpoints(&self, override_base: Option<&Url>) -> Result<(), GwsError> {
        validate_api_base_with(&self.root_url, override_base)?;
        if let Some(mtls) = &self.mtls_root_url {
            validate_api_base_with(mtls, override_base)?;
        }
        if let Some(base) = &self.base_url {
            validate_api_base_with(base, override_base)?;
        }
        validate_relative_path(&self.service_path, "servicePath")?;
        if let Some(batch) = &self.batch_path {
            validate_relative_path(batch, "batchPath")?;
        }
        for resource in self.resources.values() {
            resource.validate_paths()?;
        }
        Ok(())
    }
}

/// Reject paths that could alter the request origin or smuggle a query.
fn validate_relative_path(path: &str, what: &str) -> Result<(), GwsError> {
    let bad = path.contains("://")
        || path.starts_with("//")
        || path.contains('\\')
        || path.contains('?')
        || path.contains('#')
        || path.split('/').any(|seg| seg == "..")
        || path
            .chars()
            .any(|c| c.is_control() || is_dangerous_unicode(c));
    if bad {
        return Err(GwsError::Discovery(format!(
            "Discovery Document has an unsafe {what}: '{}'",
            path.escape_debug()
        )));
    }
    Ok(())
}

impl RestResource {
    fn validate_paths(&self) -> Result<(), GwsError> {
        for method in self.methods.values() {
            validate_relative_path(&method.path, "method path")?;
            if let Some(flat) = &method.flat_path {
                validate_relative_path(flat, "method flatPath")?;
            }
            for proto in [method.simple_upload_path(), method.resumable_upload_path()]
                .into_iter()
                .flatten()
            {
                validate_relative_path(proto, "media upload path")?;
            }
        }
        for sub in self.resources.values() {
            sub.validate_paths()?;
        }
        Ok(())
    }
}

impl RestMethod {
    fn upload_protocols(&self) -> Option<&MediaUploadProtocols> {
        self.media_upload.as_ref()?.protocols.as_ref()
    }

    /// Path for simple/multipart media uploads, e.g. `/upload/drive/v3/files`.
    pub fn simple_upload_path(&self) -> Option<&str> {
        self.upload_protocols()?
            .simple
            .as_ref()
            .map(|p| p.path.as_str())
    }

    /// Path for resumable media uploads, e.g. `/resumable/upload/drive/v3/files`
    /// (Drive) or `/upload/gmail/v1/users/{userId}/messages/send` (Gmail).
    pub fn resumable_upload_path(&self) -> Option<&str> {
        self.upload_protocols()?
            .resumable
            .as_ref()
            .map(|p| p.path.as_str())
    }

    /// Whether the method supports `uploadType=resumable`.
    pub fn supports_resumable_upload(&self) -> bool {
        self.supports_media_upload && self.resumable_upload_path().is_some()
    }

    /// Maximum upload size in bytes from `mediaUpload.maxSize` (units are
    /// binary: `KB` = 1024). `Ok(None)` when the document declares no limit.
    pub fn max_upload_size_bytes(&self) -> Result<Option<u64>, GwsError> {
        match self
            .media_upload
            .as_ref()
            .and_then(|m| m.max_size.as_deref())
        {
            None => Ok(None),
            Some(raw) => parse_size(raw).map(Some).ok_or_else(|| {
                GwsError::Discovery(format!(
                    "Discovery Document has an unparseable mediaUpload.maxSize '{}'",
                    raw.escape_debug()
                ))
            }),
        }
    }

    /// The request body schema, resolved through the document's `schemas`.
    pub fn request_schema<'a>(&self, doc: &'a RestDescription) -> Option<&'a JsonSchema> {
        doc.schema(self.request.as_ref()?.schema_ref.as_deref()?)
    }

    /// The response schema, resolved through the document's `schemas`.
    pub fn response_schema<'a>(&self, doc: &'a RestDescription) -> Option<&'a JsonSchema> {
        doc.schema(self.response.as_ref()?.schema_ref.as_deref()?)
    }

    /// Pagination metadata, or `None` if the method is not paginated.
    ///
    /// A method is paginated when it accepts `pageToken` (as a query/path
    /// parameter, or as a property of its request body schema) and its
    /// response schema declares `nextPageToken`. When the response schema is
    /// not present in the document, `nextPageToken` is assumed.
    pub fn pagination(&self, doc: &RestDescription) -> Option<Pagination> {
        let token_location = if self
            .parameters
            .get(PAGE_TOKEN)
            .is_some_and(|p| p.location.as_deref() != Some("path"))
        {
            PageTokenLocation::Query
        } else if self
            .request_schema(doc)
            .is_some_and(|s| s.properties.contains_key(PAGE_TOKEN))
        {
            PageTokenLocation::Body
        } else {
            return None;
        };
        if let Some(resp) = self.response_schema(doc)
            && !resp.properties.contains_key(NEXT_PAGE_TOKEN)
        {
            return None;
        }
        Some(Pagination {
            token_location,
            request_field: PAGE_TOKEN,
            response_field: NEXT_PAGE_TOKEN,
        })
    }
}

/// Parse a Discovery size string such as `5120GB`, `35MB`, `1024`, `2G`.
fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (digits, unit) = raw.split_at(split);
    let value: u64 = digits.parse().ok()?;
    let multiplier: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1 << 10,
        "M" | "MB" => 1 << 20,
        "G" | "GB" => 1 << 30,
        "T" | "TB" => 1 << 40,
        _ => return None,
    };
    value.checked_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_rest_description() {
        let json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/",
            "resources": {
                "files": {
                    "methods": {
                        "list": {
                            "httpMethod": "GET",
                            "path": "files",
                            "response": { "$ref": "FileList" }
                        }
                    }
                }
            },
            "schemas": {
                "FileList": {
                    "id": "FileList",
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "$ref": "File" }
                        }
                    }
                }
            }
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.name, "drive");
        assert_eq!(doc.version, "v3");
        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(doc.service_path, "drive/v3/");

        // precise resource checking
        let files = doc.resources.get("files").expect("files resource missing");
        let list = files.methods.get("list").expect("list method missing");
        assert_eq!(list.http_method, "GET");
        assert_eq!(list.path, "files");

        // schema checking
        let file_list = doc
            .schemas
            .get("FileList")
            .expect("FileList schema missing");
        assert_eq!(file_list.id.as_deref(), Some("FileList"));
    }

    #[test]
    fn test_deserialize_defaults() {
        let json = r#"{
            "name": "admin",
            "version": "directory_v1",
            "rootUrl": "https://admin.googleapis.com/"
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.service_path, ""); // default empty string
        assert!(doc.resources.is_empty());
        assert!(doc.schemas.is_empty());
    }

    const PAGED_DOC: &str = r#"{
        "name": "driveactivity", "version": "v2",
        "rootUrl": "https://driveactivity.googleapis.com/",
        "mtlsRootUrl": "https://driveactivity.mtls.googleapis.com/",
        "servicePath": "", "batchPath": "batch",
        "resources": {
            "activity": { "methods": {
                "query": { "httpMethod": "POST", "path": "v2/activity:query",
                    "request": { "$ref": "QueryReq" }, "response": { "$ref": "QueryResp" } }
            }},
            "files": { "methods": {
                "list": { "httpMethod": "GET", "path": "files",
                    "parameters": { "pageToken": { "type": "string", "location": "query" } },
                    "response": { "$ref": "FileList" } },
                "get": { "httpMethod": "GET", "path": "files/{fileId}",
                    "response": { "$ref": "File" } },
                "watch": { "httpMethod": "POST", "path": "files/watch",
                    "parameters": { "pageToken": { "type": "string", "location": "query" } },
                    "response": { "$ref": "File" } },
                "create": { "httpMethod": "POST", "path": "files",
                    "supportsMediaUpload": true,
                    "mediaUpload": { "accept": ["*/*"], "maxSize": "5120GB",
                        "protocols": {
                            "simple": { "multipart": true, "path": "/upload/drive/v3/files" },
                            "resumable": { "multipart": true, "path": "/resumable/upload/drive/v3/files" }
                        } } }
            }}
        },
        "schemas": {
            "QueryReq": { "id": "QueryReq", "type": "object",
                "properties": { "pageToken": { "type": "string" }, "filter": { "type": "string" } } },
            "QueryResp": { "id": "QueryResp", "type": "object",
                "properties": { "nextPageToken": { "type": "string" } } },
            "FileList": { "id": "FileList", "type": "object",
                "properties": { "nextPageToken": { "type": "string" } } },
            "File": { "id": "File", "type": "object", "properties": { "id": { "type": "string" } } }
        }
    }"#;

    fn paged_doc() -> RestDescription {
        serde_json::from_str(PAGED_DOC).unwrap()
    }

    fn method<'a>(doc: &'a RestDescription, resource: &str, name: &str) -> &'a RestMethod {
        &doc.resources[resource].methods[name]
    }

    #[test]
    fn pagination_detects_query_and_body_tokens() {
        let doc = paged_doc();
        let q = method(&doc, "files", "list").pagination(&doc).unwrap();
        assert_eq!(q.token_location, PageTokenLocation::Query);
        assert_eq!(
            (q.request_field, q.response_field),
            ("pageToken", "nextPageToken")
        );
        let b = method(&doc, "activity", "query").pagination(&doc).unwrap();
        assert_eq!(b.token_location, PageTokenLocation::Body);
    }

    #[test]
    fn pagination_absent_without_tokens() {
        let doc = paged_doc();
        assert!(method(&doc, "files", "get").pagination(&doc).is_none());
        // Accepts pageToken but the response has no nextPageToken.
        assert!(method(&doc, "files", "watch").pagination(&doc).is_none());
    }

    #[test]
    fn upload_protocol_accessors() {
        let doc = paged_doc();
        let create = method(&doc, "files", "create");
        assert_eq!(create.simple_upload_path(), Some("/upload/drive/v3/files"));
        assert_eq!(
            create.resumable_upload_path(),
            Some("/resumable/upload/drive/v3/files")
        );
        assert!(create.supports_resumable_upload());
        assert_eq!(
            create.max_upload_size_bytes().unwrap(),
            Some(5120 * (1 << 30))
        );
        let get = method(&doc, "files", "get");
        assert!(!get.supports_resumable_upload());
        assert_eq!(get.max_upload_size_bytes().unwrap(), None);
    }

    #[test]
    fn schema_accessors_resolve_refs() {
        let doc = paged_doc();
        let q = method(&doc, "activity", "query");
        assert_eq!(
            q.request_schema(&doc).unwrap().id.as_deref(),
            Some("QueryReq")
        );
        assert_eq!(
            q.response_schema(&doc).unwrap().id.as_deref(),
            Some("QueryResp")
        );
        assert!(
            method(&doc, "files", "create")
                .request_schema(&doc)
                .is_none()
        );
    }

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("35MB"), Some(35 << 20));
        assert_eq!(parse_size("2G"), Some(2 << 30));
        assert_eq!(parse_size("1kb"), Some(1024));
        assert_eq!(parse_size("5PB"), None);
        assert_eq!(parse_size("MB"), None);
        assert_eq!(parse_size("99999999999999TB"), None);
        let bad = RestMethod {
            media_upload: Some(MediaUpload {
                max_size: Some("lots".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(matches!(
            bad.max_upload_size_bytes(),
            Err(GwsError::Discovery(_))
        ));
    }

    #[test]
    fn validate_endpoints_accepts_google_docs() {
        let doc = paged_doc();
        doc.validate_endpoints(None).unwrap();
        assert_eq!(
            doc.mtls_root_url.as_deref(),
            Some("https://driveactivity.mtls.googleapis.com/")
        );
        assert_eq!(doc.batch_path.as_deref(), Some("batch"));
        assert_eq!(
            doc.api_root(None).unwrap().as_str(),
            "https://driveactivity.googleapis.com/"
        );
        let base = Url::parse("https://proxy.example/").unwrap();
        assert_eq!(doc.api_root(Some(&base)).unwrap(), base);
    }

    #[test]
    fn service_base_prefers_base_url_unless_overridden() {
        let mut doc: RestDescription = serde_json::from_str(PAGED_DOC).unwrap();
        doc.service_path = "v2/".to_string();
        assert_eq!(
            doc.service_base(None).unwrap(),
            "https://driveactivity.googleapis.com/v2/"
        );
        doc.base_url = Some("https://driveactivity.googleapis.com/base/".to_string());
        assert_eq!(
            doc.service_base(None).unwrap(),
            "https://driveactivity.googleapis.com/base/"
        );
        let proxy = Url::parse("http://127.0.0.1:9/").unwrap();
        assert_eq!(
            doc.service_base(Some(&proxy)).unwrap(),
            "http://127.0.0.1:9/v2/"
        );
        doc.base_url = Some("https://evil.example/".to_string());
        assert!(doc.service_base(None).is_err());
    }

    #[test]
    fn validate_endpoints_rejects_untrusted_fields() {
        let cases: &[(&str, &str)] = &[
            (
                "\"https://driveactivity.googleapis.com/\"",
                "\"http://driveactivity.googleapis.com/\"",
            ),
            (
                "\"https://driveactivity.googleapis.com/\"",
                "\"https://evil.example/\"",
            ),
            (
                "\"https://driveactivity.mtls.googleapis.com/\"",
                "\"https://evil.example/\"",
            ),
            (
                "\"servicePath\": \"\"",
                "\"servicePath\": \"//evil.example/\"",
            ),
            ("\"servicePath\": \"\"", "\"servicePath\": \"../x/\""),
            (
                "\"batchPath\": \"batch\"",
                "\"batchPath\": \"https://evil.example/b\"",
            ),
            (
                "\"path\": \"files/{fileId}\"",
                "\"path\": \"https://evil.example/{fileId}\"",
            ),
            ("\"path\": \"files/watch\"", "\"path\": \"files/watch?x=1\""),
            ("\"/upload/drive/v3/files\"", "\"//evil.example/upload\""),
            (
                "\"/resumable/upload/drive/v3/files\"",
                "\"/resumable\\\\evil\"",
            ),
        ];
        for (from, to) in cases {
            assert!(PAGED_DOC.contains(from), "fixture missing {from}");
            let doc: RestDescription =
                serde_json::from_str(&PAGED_DOC.replacen(from, to, 1)).unwrap();
            assert!(
                doc.validate_endpoints(None).is_err(),
                "{to} should be rejected"
            );
        }
        let proxied = PAGED_DOC.replace(
            "https://driveactivity.googleapis.com/",
            "https://proxy.example/",
        );
        let doc: RestDescription = serde_json::from_str(&proxied).unwrap();
        assert!(doc.validate_endpoints(None).is_err());
        let base = Url::parse("https://proxy.example/").unwrap();
        doc.validate_endpoints(Some(&base)).unwrap();
        assert!(doc.api_root(None).is_err());
    }
}
