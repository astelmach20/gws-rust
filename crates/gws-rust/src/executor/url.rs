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

//! Request URL construction from Discovery path templates.
//!
//! This is the only place request URLs are assembled. Every URL built here
//! is later checked by [`EndpointPolicy`] before a
//! credential is attached.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::validate::EndpointPolicy;

use crate::discovery::{RestDescription, RestMethod};
use crate::error::GwsError;

/// Which endpoint of a method to address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UrlTarget {
    /// The regular REST endpoint.
    Method,
    /// The simple/multipart media upload endpoint.
    SimpleUpload,
    /// The resumable media upload endpoint.
    ResumableUpload,
}

/// A request URL plus its query parameters (kept separate so the page token
/// and other per-request values can be appended).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestUrl {
    pub url: String,
    pub query: Vec<(String, String)>,
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Choose between `flatPath` and `path`.
///
/// Prefer `flatPath` when its placeholders match the method's path
/// parameters. Some documents (Slides `presentations.get`) have `flatPath`
/// placeholders that don't match parameter names; fall back to `path`,
/// whose RFC 6570 `{+var}` operators are handled below.
pub(crate) fn path_template(method: &RestMethod) -> &str {
    match method.flat_path.as_deref() {
        Some(fp) => {
            let all_match = method
                .parameters
                .iter()
                .filter(|(_, p)| p.location.as_deref() == Some("path"))
                .all(|(name, _)| {
                    fp.contains(&format!("{{{name}}}")) || fp.contains(&format!("{{+{name}}}"))
                });
            if all_match { fp } else { method.path.as_str() }
        }
        None => method.path.as_str(),
    }
}

/// Build the URL and query parameters for `method`.
pub(crate) fn build_url(
    doc: &RestDescription,
    method: &RestMethod,
    params: &Map<String, Value>,
    target: UrlTarget,
    endpoints: &EndpointPolicy,
) -> Result<RequestUrl, GwsError> {
    let template = path_template(method);
    let path_parameters = extract_template_path_parameters(template);
    let mut query: Vec<(String, String)> = Vec::new();

    for (key, value) in params {
        if path_parameters.contains(key.as_str()) {
            continue;
        }
        let def = method.parameters.get(key);
        if def.and_then(|p| p.location.as_deref()) == Some("path") {
            return Err(GwsError::Validation(format!(
                "Path parameter '{key}' was provided but is not present in URL template '{template}'"
            )));
        }
        match value {
            // Repeated parameters (validated earlier) expand into one entry each.
            Value::Array(items) => {
                for item in items {
                    query.push((key.clone(), scalar_to_string(item)));
                }
            }
            other => query.push((key.clone(), scalar_to_string(other))),
        }
    }

    let url =
        match target {
            UrlTarget::Method => {
                let path = render_path_template(template, params)?;
                format!("{}{}", doc.service_base(endpoints.override_base())?, path)
            }
            UrlTarget::SimpleUpload | UrlTarget::ResumableUpload => {
                let upload_template = match target {
                UrlTarget::ResumableUpload => method.resumable_upload_path(),
                _ => method.simple_upload_path(),
            }
            .ok_or_else(|| {
                GwsError::Validation(format!(
                    "Method {} does not advertise a {} upload endpoint in its Discovery Document",
                    method.id.as_deref().unwrap_or("(unknown)"),
                    if target == UrlTarget::ResumableUpload { "resumable" } else { "simple" }
                ))
            })?;
                let upload_path = render_path_template(upload_template, params)?;
                format!(
                    "{}{}",
                    doc.api_root(endpoints.override_base())?,
                    upload_path.trim_start_matches('/')
                )
            }
        };

    Ok(RequestUrl { url, query })
}

pub(crate) fn extract_template_path_parameters(path_template: &str) -> HashSet<&str> {
    let mut found = HashSet::new();
    let mut cursor = 0;
    while let Some(open_idx) = path_template[cursor..].find('{') {
        let token_start = cursor + open_idx;
        let Some(close_idx) = path_template[token_start..].find('}') else {
            break;
        };
        let token_end = token_start + close_idx;
        let token = &path_template[token_start + 1..token_end];
        found.insert(token.strip_prefix('+').unwrap_or(token));
        cursor = token_end + 1;
    }
    found
}

pub(crate) fn render_path_template(
    path_template: &str,
    params: &Map<String, Value>,
) -> Result<String, GwsError> {
    let mut rendered = String::with_capacity(path_template.len());
    let mut cursor = 0;

    while let Some(open_idx) = path_template[cursor..].find('{') {
        let token_start = cursor + open_idx;
        rendered.push_str(&path_template[cursor..token_start]);

        let Some(close_idx) = path_template[token_start..].find('}') else {
            rendered.push_str(&path_template[token_start..]);
            return Ok(rendered);
        };

        let token_end = token_start + close_idx;
        let token = &path_template[token_start + 1..token_end];
        let (is_plus, key) = match token.strip_prefix('+') {
            Some(key) => (true, key),
            None => (false, token),
        };

        match params.get(key) {
            Some(value) => {
                let val_str = scalar_to_string(value);
                let encoded = if is_plus {
                    let validated = crate::validate::validate_resource_name(&val_str)?;
                    crate::validate::encode_path_preserving_slashes(validated)
                } else {
                    crate::validate::encode_path_segment(&val_str)
                };
                rendered.push_str(&encoded);
            }
            None => {
                return Err(GwsError::Validation(format!(
                    "Missing value for path parameter '{key}' in URL template '{path_template}'. Provide it via --params"
                )));
            }
        }
        cursor = token_end + 1;
    }

    rendered.push_str(&path_template[cursor..]);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{
        MediaUpload, MediaUploadProtocol, MediaUploadProtocols, MethodParameter,
    };
    use serde_json::json;
    use std::collections::HashMap;

    fn doc_with_base(base: &str) -> RestDescription {
        RestDescription {
            base_url: Some(base.to_string()),
            ..Default::default()
        }
    }

    fn path_params(names: &[&str]) -> HashMap<String, MethodParameter> {
        names
            .iter()
            .map(|n| {
                (
                    (*n).to_string(),
                    MethodParameter {
                        location: Some("path".to_string()),
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    fn method(path: &str, params: HashMap<String, MethodParameter>) -> RestMethod {
        RestMethod {
            path: path.to_string(),
            flat_path: Some(path.to_string()),
            parameters: params,
            ..Default::default()
        }
    }

    fn google() -> EndpointPolicy {
        EndpointPolicy::google_only()
    }

    #[test]
    fn basic_and_substitution() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method("files/{fileId}", path_params(&["fileId"]));
        let mut params = Map::new();
        params.insert("fileId".into(), json!("123"));
        params.insert("q".into(), json!("search term"));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(u.url, "https://api.googleapis.com/files/123");
        assert_eq!(u.query, vec![("q".to_string(), "search term".to_string())]);
    }

    #[test]
    fn repeated_query_param_expands_array() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method("messages", HashMap::new());
        let mut params = Map::new();
        params.insert("metadataHeaders".into(), json!(["Subject", "Date"]));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(
            u.query,
            vec![
                ("metadataHeaders".to_string(), "Subject".to_string()),
                ("metadataHeaders".to_string(), "Date".to_string()),
            ]
        );
    }

    #[test]
    fn encodes_path_parameter_chars() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method(
            "spreadsheets/{spreadsheetId}/values/{range}",
            path_params(&["spreadsheetId", "range"]),
        );
        let mut params = Map::new();
        params.insert("spreadsheetId".into(), json!("abc123"));
        params.insert("range".into(), json!("hash#1!A1:B2"));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(
            u.url,
            "https://api.googleapis.com/spreadsheets/abc123/values/hash%231%21A1%3AB2"
        );
    }

    #[test]
    fn plus_expansion() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method("v1/{+name}", path_params(&["name"]));
        let mut params = Map::new();
        params.insert("name".into(), json!("projects/p1/locations/us/topics/t1"));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(
            u.url,
            "https://api.googleapis.com/v1/projects/p1/locations/us/topics/t1"
        );

        params.insert("name".into(), json!("projects/p1#frag?x=y"));
        let err = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap_err();
        assert!(err.to_string().contains("must not contain '?' or '#'"));

        params.insert("name".into(), json!("projects/../../etc/passwd"));
        let err = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    #[test]
    fn upload_endpoints() {
        let doc = RestDescription {
            root_url: "https://www.googleapis.com/".to_string(),
            ..Default::default()
        };
        let mut m = method("drive/v3/files/{fileId}", path_params(&["fileId"]));
        m.media_upload = Some(MediaUpload {
            protocols: Some(MediaUploadProtocols {
                simple: Some(MediaUploadProtocol {
                    path: "/upload/drive/v3/files/{fileId}".to_string(),
                    multipart: Some(true),
                }),
                resumable: Some(MediaUploadProtocol {
                    path: "/resumable/upload/drive/v3/files/{fileId}".to_string(),
                    multipart: Some(true),
                }),
            }),
            ..Default::default()
        });
        let mut params = Map::new();
        params.insert("fileId".into(), json!("abc/123"));
        let simple = build_url(&doc, &m, &params, UrlTarget::SimpleUpload, &google()).unwrap();
        assert_eq!(
            simple.url,
            "https://www.googleapis.com/upload/drive/v3/files/abc%2F123"
        );
        let resumable =
            build_url(&doc, &m, &params, UrlTarget::ResumableUpload, &google()).unwrap();
        assert_eq!(
            resumable.url,
            "https://www.googleapis.com/resumable/upload/drive/v3/files/abc%2F123"
        );
    }

    #[test]
    fn override_replaces_root_and_base() {
        let doc = RestDescription {
            root_url: "https://www.googleapis.com/".to_string(),
            service_path: "drive/v3/".to_string(),
            base_url: Some("https://www.googleapis.com/drive/v3/".to_string()),
            ..Default::default()
        };
        let endpoints = EndpointPolicy::with_override("http://127.0.0.1:9999").unwrap();
        let m = method("files", HashMap::new());
        let u = build_url(&doc, &m, &Map::new(), UrlTarget::Method, &endpoints).unwrap();
        assert_eq!(u.url, "http://127.0.0.1:9999/drive/v3/files");
    }

    #[test]
    fn placeholder_like_values_are_not_substituted() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method("v1/{parent}/{child}", HashMap::new());
        let mut params = Map::new();
        params.insert("parent".into(), json!("literal-{child}-value"));
        params.insert("child".into(), json!("ok"));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(
            u.url,
            "https://api.googleapis.com/v1/literal-%7Bchild%7D-value/ok"
        );
    }

    #[test]
    fn errors_for_path_param_not_in_template() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let mut m = method("files", path_params(&["fileId"]));
        m.flat_path = Some("files".into());
        let mut params = Map::new();
        params.insert("fileId".into(), json!("123"));
        let err = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap_err();
        assert!(err.to_string().contains("was provided but is not present"));
    }

    #[test]
    fn missing_path_value_is_an_error_not_a_literal_placeholder() {
        let doc = doc_with_base("https://api.googleapis.com/");
        let m = method("files/{fileId}", path_params(&["fileId"]));
        let err = build_url(&doc, &m, &Map::new(), UrlTarget::Method, &google()).unwrap_err();
        assert!(
            err.to_string()
                .contains("Missing value for path parameter 'fileId'")
        );
    }

    #[test]
    fn flatpath_fallback_on_mismatch() {
        let doc = doc_with_base("https://slides.googleapis.com/");
        let m = RestMethod {
            path: "v1/presentations/{+presentationId}".to_string(),
            flat_path: Some("v1/presentations/{presentationsId}".to_string()),
            parameters: path_params(&["presentationId"]),
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("presentationId".into(), json!("abc123"));
        let u = build_url(&doc, &m, &params, UrlTarget::Method, &google()).unwrap();
        assert_eq!(
            u.url,
            "https://slides.googleapis.com/v1/presentations/abc123"
        );
    }
}
