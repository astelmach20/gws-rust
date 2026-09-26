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

//! JSON Schema Validation & Reference Resolution
//!
//! Provides utilities to validate JSON payloads against the Google API Discovery Document
//! schemas before dispatching requests. This ensures immediate client-side feedback
//! for invalid API payloads.

use serde_json::{Value, json};

use crate::discovery::{
    JsonSchema, MethodParameter, PageTokenLocation, RestDescription, RestMethod, RestResource,
    fetch_discovery_document,
};
use crate::error::GwsError;
use crate::services::resolve_service_spec;

/// Handles the `gwsr schema <dotted.path>` command and returns the schema as
/// JSON (the caller formats and prints it).
///
/// Path format: `service.resource[.subresource].method`, `service.Type`, or
/// `service.method` for a method declared outside any resource,
/// where `service` is a registered alias, `alias:version`, or any Discovery
/// API as `<api>:<version>`.
/// Examples: `drive.files.list`, `drive.File`, `admin:directory_v1.users.list`.
pub async fn handle_schema_command(path: &str, resolve_refs: bool) -> Result<Value, GwsError> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(GwsError::Validation(format!(
            "Schema path must be 'service.Type' or 'service.resource.method' (e.g. 'drive.files.list', 'youtube:v3.videos.list'), got '{path}'"
        )));
    }

    let service_name = parts[0];
    let resolved = resolve_service_spec(service_name, None)?;
    let doc = fetch_discovery_document(&resolved.api_name, &resolved.version).await?;

    // Case 1: Schema lookup (e.g., "drive.File")
    if parts.len() == 2 {
        let schema_name = parts[1];
        if let Some(schema) = doc.schemas.get(schema_name) {
            let mut output = schema_to_json(schema);
            if resolve_refs {
                let mut seen = std::collections::HashSet::new();
                // Add self to seen to prevent immediate recursion
                seen.insert(schema_name.to_string());
                resolve_schema_refs(&mut output, &doc, &mut seen);
            }
            return Ok(output);
        }
        // A method declared outside any resource (e.g. `oauth2:v2.tokeninfo`).
        if let Some(method) = doc.methods.get(schema_name) {
            let mut output = build_schema_output(&doc, method);
            if resolve_refs {
                let mut seen = std::collections::HashSet::new();
                resolve_schema_refs(&mut output, &doc, &mut seen);
            }
            return Ok(output);
        }
        if doc.resources.contains_key(schema_name) {
            return Err(GwsError::Validation(format!(
                "'{schema_name}' is a resource. To see its methods, try 'gwsr schema {service_name}.{schema_name}.list' (or similar). To see a type definition, try 'gwsr schema {service_name}.<Type>'."
            )));
        }
        let mut available: Vec<&String> = doc.schemas.keys().collect();
        available.sort();
        return Err(GwsError::Validation(format!(
            "Schema or resource '{schema_name}' not found. Available schemas: {available:?}"
        )));
    }

    // Case 2: Method lookup (e.g., "drive.files.list")
    let resource_path = &parts[1..parts.len() - 1];
    let method_name = parts[parts.len() - 1];

    let method = find_method(&doc, resource_path, method_name)?;

    let mut output = build_schema_output(&doc, method);
    if resolve_refs {
        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut output, &doc, &mut seen);
    }
    Ok(output)
}

/// The `gwsr schema` argument that shows `method` of `doc`, e.g.
/// `events.subscriptions.list` or `youtube:v3.videos.list`.
///
/// Built from the method's position in the resource tree, not from its
/// Discovery `id`: ids use the API's own name (`workspaceevents`,
/// `groupsSettings`) and sometimes skip resources (`alertcenter.getSettings`
/// lives under the `v1beta1` resource), so they are not valid schema paths.
/// `None` when `method` is not part of `doc`.
pub fn method_schema_path(doc: &RestDescription, method: &RestMethod) -> Option<String> {
    fn walk(
        resources: &std::collections::HashMap<String, RestResource>,
        method: &RestMethod,
        path: &mut Vec<String>,
    ) -> bool {
        for (name, resource) in resources {
            path.push(name.clone());
            if let Some((method_name, _)) = resource
                .methods
                .iter()
                .find(|(_, m)| std::ptr::eq(*m, method))
            {
                path.push(method_name.clone());
                return true;
            }
            if walk(&resource.resources, method, path) {
                return true;
            }
            path.pop();
        }
        false
    }

    let mut path = Vec::new();
    // A method declared outside any resource (e.g. `oauth2:v2.tokeninfo`).
    if let Some((method_name, _)) = doc.methods.iter().find(|(_, m)| std::ptr::eq(*m, method)) {
        path.push(method_name.clone());
    } else if !walk(&doc.resources, method, &mut path) {
        return None;
    }
    let service = crate::services::SERVICES
        .iter()
        .find(|e| e.api_name == doc.name && e.version == doc.version)
        .map(|e| e.aliases[0].to_string())
        .unwrap_or_else(|| format!("{}:{}", doc.name, doc.version));
    Some(format!("{service}.{}", path.join(".")))
}

/// Walks the resource tree to find a method.
fn find_method<'a>(
    doc: &'a RestDescription,
    resource_path: &[&str],
    method_name: &str,
) -> Result<&'a RestMethod, GwsError> {
    if resource_path.is_empty() {
        return Err(GwsError::Validation(
            "Resource path cannot be empty".to_string(),
        ));
    }

    let first_resource_name = resource_path[0];
    let resource = doc.resources.get(first_resource_name).ok_or_else(|| {
        let mut available: Vec<&String> = doc.resources.keys().collect();
        available.sort();
        GwsError::Validation(format!(
            "Resource '{first_resource_name}' not found. Available resources: {available:?}"
        ))
    })?;

    // Walk deeper into sub-resources
    let mut current_resource: &RestResource = resource;
    for &sub_name in &resource_path[1..] {
        current_resource = current_resource.resources.get(sub_name).ok_or_else(|| {
            let mut available: Vec<&String> = current_resource.resources.keys().collect();
            available.sort();
            GwsError::Validation(format!(
                "Sub-resource '{sub_name}' not found. Available: {available:?}"
            ))
        })?;
    }

    current_resource.methods.get(method_name).ok_or_else(|| {
        let mut available: Vec<&String> = current_resource.methods.keys().collect();
        available.sort();
        GwsError::Validation(format!(
            "Method '{method_name}' not found. Available methods: {available:?}"
        ))
    })
}

/// Builds the schema output JSON for a method.
fn build_schema_output(doc: &RestDescription, method: &RestMethod) -> Value {
    let mut params = json!({});
    for (name, param) in &method.parameters {
        params[name] = param_to_json(param);
    }

    let mut output = json!({
        "httpMethod": method.http_method,
        "path": method.path,
        "description": method.description.as_deref().unwrap_or(""),
        "parameters": params,
        "scopes": method.scopes,
    });

    if !method.parameter_order.is_empty() {
        output["parameterOrder"] = json!(method.parameter_order);
    }

    // Resolve request body schema
    if let Some(ref req_ref) = method.request
        && let Some(ref schema_name) = req_ref.schema_ref
    {
        output["requestBody"] = json!({
            "schemaRef": schema_name,
        });
        if let Some(schema) = doc.schemas.get(schema_name) {
            output["requestBody"]["schema"] = schema_to_json(schema);
        }
    }

    // Response schema ref
    if let Some(ref resp_ref) = method.response
        && let Some(ref schema_name) = resp_ref.schema_ref
    {
        output["response"] = json!({
            "schemaRef": schema_name,
        });
        // Also inline the response schema structure if available
        if let Some(schema) = doc.schemas.get(schema_name) {
            output["response"]["schema"] = schema_to_json(schema);
        }
    }

    if let Some(pagination) = method.pagination(doc) {
        let location = match pagination.token_location {
            PageTokenLocation::Body => "body",
            _ => "query",
        };
        output["pagination"] = json!({
            "tokenLocation": location,
            "requestField": pagination.request_field,
            "responseField": pagination.response_field,
        });
    }

    if method.supports_media_upload {
        let mut upload = json!({});
        if let Some(path) = method.simple_upload_path() {
            upload["simplePath"] = json!(path);
        }
        if let Some(path) = method.resumable_upload_path() {
            upload["resumablePath"] = json!(path);
        }
        if let Some(media) = &method.media_upload {
            if let Some(max) = &media.max_size {
                upload["maxSize"] = json!(max);
            }
            if let Some(accept) = &media.accept {
                upload["accept"] = json!(accept);
            }
        }
        output["mediaUpload"] = upload;
    }

    output
}

fn param_to_json(param: &MethodParameter) -> Value {
    let mut p = json!({
        "type": param.param_type.as_deref().unwrap_or("string"),
        "required": param.required,
    });

    if let Some(ref loc) = param.location {
        p["location"] = json!(loc);
    }
    if let Some(ref desc) = param.description {
        p["description"] = json!(desc);
    }
    if let Some(ref fmt) = param.format {
        p["format"] = json!(fmt);
    }
    if let Some(ref def) = param.default {
        p["default"] = json!(def);
    }
    if let Some(ref vals) = param.enum_values {
        p["enum"] = json!(vals);
    }
    if let Some(ref pattern) = param.pattern {
        p["pattern"] = json!(pattern);
    }
    if param.repeated {
        p["repeated"] = json!(true);
    }
    if param.deprecated {
        p["deprecated"] = json!(true);
    }

    p
}

fn schema_to_json(schema: &JsonSchema) -> Value {
    let mut s = json!({});

    if let Some(ref t) = schema.schema_type {
        s["type"] = json!(t);
    }
    if let Some(ref desc) = schema.description {
        s["description"] = json!(desc);
    }

    if !schema.properties.is_empty() {
        let mut props = json!({});
        for (name, prop) in &schema.properties {
            let mut p = json!({});
            if let Some(ref t) = prop.prop_type {
                p["type"] = json!(t);
            }
            if let Some(ref r) = prop.schema_ref {
                p["$ref"] = json!(r);
            }
            if let Some(ref desc) = prop.description {
                p["description"] = json!(desc);
            }
            if prop.read_only {
                p["readOnly"] = json!(true);
            }
            if let Some(ref fmt) = prop.format {
                p["format"] = json!(fmt);
            }

            // Handle items for array types
            if let Some(ref items) = prop.items {
                let mut items_json = json!({});
                if let Some(ref t) = items.prop_type {
                    items_json["type"] = json!(t);
                }
                if let Some(ref r) = items.schema_ref {
                    items_json["$ref"] = json!(r);
                }
                p["items"] = items_json;
            }

            props[name] = p;
        }
        s["properties"] = props;
    }

    s
}

/// Recursively resolves "$ref" fields in the JSON value.
fn resolve_schema_refs(
    val: &mut Value,
    doc: &RestDescription,
    seen: &mut std::collections::HashSet<String>,
) {
    match val {
        Value::Object(map) => {
            // Check if this object is a reference
            if let Some(ref_name) = map
                .get("$ref")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            {
                // If we haven't seen this schema yet in this branch
                if !seen.contains(&ref_name)
                    && let Some(schema) = doc.schemas.get(&ref_name)
                {
                    seen.insert(ref_name.clone());
                    let mut resolved = schema_to_json(schema);
                    // Recursively resolve the resolved schema
                    resolve_schema_refs(&mut resolved, doc, seen);
                    seen.remove(&ref_name);

                    // Merge resolved schema into current object, but preserve existing fields
                    // (though usually $ref stands alone)
                    if let Value::Object(resolved_map) = resolved {
                        for (k, v) in resolved_map {
                            map.entry(k).or_insert(v);
                        }
                    }
                }
            }

            // Recurse into all fields
            for (_, v) in map.iter_mut() {
                resolve_schema_refs(v, doc, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                resolve_schema_refs(v, doc, seen);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_nested_method(name: &str, version: &str) -> RestDescription {
        let mut messages = RestResource::default();
        messages.methods.insert(
            "create".into(),
            RestMethod {
                id: Some(format!("{name}.spaces.messages.create")),
                ..Default::default()
            },
        );
        let mut spaces = RestResource::default();
        spaces.resources.insert("messages".into(), messages);
        let mut doc = RestDescription {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        };
        doc.resources.insert("spaces".into(), spaces);
        doc
    }

    #[test]
    fn method_schema_path_follows_the_resource_tree() {
        let doc = doc_with_nested_method("workspaceevents", "v1");
        let m = &doc.resources["spaces"].resources["messages"].methods["create"];
        // Registered services use their primary alias...
        assert_eq!(
            method_schema_path(&doc, m).as_deref(),
            Some("events.spaces.messages.create")
        );
        // ...other APIs and versions use `<api>:<version>`.
        for (name, version, expected) in [
            ("youtube", "v3", "youtube:v3.spaces.messages.create"),
            (
                "workspaceevents",
                "v2",
                "workspaceevents:v2.spaces.messages.create",
            ),
        ] {
            let doc = doc_with_nested_method(name, version);
            let m = &doc.resources["spaces"].resources["messages"].methods["create"];
            assert_eq!(method_schema_path(&doc, m).as_deref(), Some(expected));
        }
    }

    #[test]
    fn method_schema_path_names_top_level_methods() {
        let mut doc = doc_with_nested_method("oauth2", "v2");
        doc.methods.insert(
            "tokeninfo".into(),
            RestMethod {
                id: Some("oauth2.tokeninfo".into()),
                ..Default::default()
            },
        );
        let m = &doc.methods["tokeninfo"];
        assert_eq!(
            method_schema_path(&doc, m).as_deref(),
            Some("oauth2:v2.tokeninfo")
        );
    }

    #[test]
    fn method_schema_path_is_none_for_a_foreign_method() {
        let doc = doc_with_nested_method("chat", "v1");
        let other = RestMethod {
            id: Some("chat.spaces.messages.create".into()),
            ..Default::default()
        };
        assert_eq!(method_schema_path(&doc, &other), None);
    }

    #[test]
    fn test_param_to_json() {
        let param = MethodParameter {
            param_type: Some("integer".to_string()),
            description: Some("desc".to_string()),
            location: Some("query".to_string()),
            required: true,
            format: Some("int32".to_string()),
            default: Some("0".to_string()),
            enum_values: Some(vec!["0".to_string(), "1".to_string()]),
            enum_descriptions: None,
            repeated: false,
            minimum: None,
            maximum: None,
            pattern: Some("^[01]$".to_string()),
            deprecated: true,
        };

        let json = param_to_json(&param);
        assert_eq!(json["type"], "integer");
        assert_eq!(json["description"], "desc");
        assert_eq!(json["location"], "query");
        assert_eq!(json["required"], true);
        assert_eq!(json["format"], "int32");
        assert_eq!(json["default"], "0");
        assert!(json["enum"].is_array());
        assert_eq!(json["pattern"], "^[01]$");
        assert_eq!(json["deprecated"], true);
        // repeated: false should NOT appear in output
        assert!(json.get("repeated").is_none());
    }

    #[test]
    fn test_param_to_json_repeated() {
        let param = MethodParameter {
            param_type: Some("string".to_string()),
            location: Some("query".to_string()),
            repeated: true,
            ..Default::default()
        };

        let json = param_to_json(&param);
        assert_eq!(json["type"], "string");
        assert_eq!(json["repeated"], true);
    }

    #[test]
    fn test_schema_to_json_basic() {
        let mut properties = std::collections::HashMap::new();
        properties.insert(
            "name".to_string(),
            crate::discovery::JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );

        let schema = JsonSchema {
            schema_type: Some("object".to_string()),
            properties,
            ..Default::default()
        };

        let json = schema_to_json(&schema);
        assert_eq!(json["type"], "object");
        assert!(json["properties"].is_object());
        assert_eq!(json["properties"]["name"]["type"], "string");
    }

    #[test]
    fn test_resolve_schema_refs_basic() {
        let mut schemas = std::collections::HashMap::new();
        let target_schema = JsonSchema {
            schema_type: Some("string".to_string()),
            description: Some("Resolved type".to_string()),
            ..Default::default()
        };
        schemas.insert("Target".to_string(), target_schema);

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let mut val = json!({
            "$ref": "Target"
        });

        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut val, &doc, &mut seen);

        assert_eq!(val["type"], "string");
        assert_eq!(val["description"], "Resolved type");
        // $ref might remain or effectively be merged, checking properties is key
    }

    #[test]
    fn test_resolve_schema_refs_nested() {
        let mut schemas = std::collections::HashMap::new();
        let child = JsonSchema {
            schema_type: Some("integer".to_string()),
            ..Default::default()
        };
        schemas.insert("Child".to_string(), child);

        let parent = JsonSchema {
            schema_type: Some("object".to_string()),
            properties: {
                let mut map = std::collections::HashMap::new();
                map.insert(
                    "f".to_string(),
                    crate::discovery::JsonSchemaProperty {
                        schema_ref: Some("Child".to_string()),
                        ..Default::default()
                    },
                );
                map
            },
            ..Default::default()
        };
        schemas.insert("Parent".to_string(), parent);

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let mut val = json!({
            "$ref": "Parent"
        });

        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut val, &doc, &mut seen);

        // Check Parent resolved
        assert_eq!(val["type"], "object");
        // Check Child resolved inside Parent
        // note: schema_to_json converts ref to $ref property, then resolve_schema_refs follows it
        // The implementation matches on "$ref" keys in objects.
        // schema_to_json for Parent produces { properties: { f: { $ref: "Child" } } }
        // The recursion should resolve f.$ref to Child content.

        let f_node = &val["properties"]["f"];
        assert_eq!(f_node["type"], "integer");
    }

    #[test]
    fn test_build_schema_output_includes_pagination_and_upload() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "resources": { "items": { "methods": {
                "search": { "httpMethod": "POST", "path": "items:search",
                    "request": { "$ref": "Req" }, "response": { "$ref": "Resp" } },
                "upload": { "httpMethod": "POST", "path": "items",
                    "supportsMediaUpload": true,
                    "mediaUpload": { "maxSize": "10MB", "accept": ["*/*"], "protocols": {
                        "simple": { "path": "/upload/x/v1/items", "multipart": true },
                        "resumable": { "path": "/resumable/upload/x/v1/items", "multipart": true } } } }
            } } },
            "schemas": {
                "Req": { "type": "object", "properties": { "pageToken": { "type": "string" } } },
                "Resp": { "type": "object", "properties": { "nextPageToken": { "type": "string" } } }
            }
        }))
        .unwrap();
        let search = find_method(&doc, &["items"], "search").unwrap();
        let out = build_schema_output(&doc, search);
        assert_eq!(out["pagination"]["tokenLocation"], "body");
        assert_eq!(out["pagination"]["responseField"], "nextPageToken");
        assert!(out.get("mediaUpload").is_none());

        let upload = find_method(&doc, &["items"], "upload").unwrap();
        let out = build_schema_output(&doc, upload);
        assert_eq!(out["mediaUpload"]["simplePath"], "/upload/x/v1/items");
        assert_eq!(
            out["mediaUpload"]["resumablePath"],
            "/resumable/upload/x/v1/items"
        );
        assert_eq!(out["mediaUpload"]["maxSize"], "10MB");
        assert!(out.get("pagination").is_none());
    }

    #[test]
    fn test_find_method_errors_list_sorted_alternatives() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "resources": { "b": {}, "a": { "methods": { "z": { "httpMethod": "GET", "path": "z" },
                                                          "y": { "httpMethod": "GET", "path": "y" } } } }
        }))
        .unwrap();
        let err = find_method(&doc, &["c"], "list").unwrap_err().to_string();
        assert!(err.contains(r#"["a", "b"]"#), "{err}");
        let err = find_method(&doc, &["a"], "list").unwrap_err().to_string();
        assert!(err.contains(r#"["y", "z"]"#), "{err}");
        assert!(find_method(&doc, &[], "list").is_err());
    }

    #[tokio::test]
    async fn test_handle_schema_command_rejects_malformed_paths() {
        for bad in ["drive", "drive.", ".files.list", "drive..list"] {
            assert!(matches!(
                handle_schema_command(bad, false).await,
                Err(GwsError::Validation(_))
            ));
        }
        // Unknown services fail before any network access, with a suggestion.
        let err = handle_schema_command("drvie.files.list", false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Did you mean 'drive'?"), "{err}");
        // Invalid <api>:<version> identifiers are rejected up front.
        assert!(matches!(
            handle_schema_command("../x:v1.a.b", false).await,
            Err(GwsError::Validation(_))
        ));
    }
}
