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
    JsonSchema, JsonSchemaProperty, MethodParameter, PageTokenLocation, RestDescription,
    RestMethod, RestResource, fetch_discovery_document,
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
/// Examples: `drive.files.list`, `drive.File`, `admin:directory_v1.users.list`,
/// `dfareporting:v3.4.ads.list` (a dotted version stays whole).
/// `api_version` (`--api-version`) wins over the `:version` suffix, as it
/// does when the method itself runs.
pub async fn handle_schema_command(
    path: &str,
    api_version: Option<&str>,
    resolve_refs: bool,
) -> Result<Value, GwsError> {
    let (service_name, rest) = split_schema_path(path)?;
    let parts: Vec<&str> = std::iter::once(service_name).chain(rest).collect();
    let resolved = resolve_service_spec(service_name, api_version)?;
    crate::service::require_discovery_api(&resolved.api_name, "schema")?;
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
        if let Some(method) = doc
            .methods
            .get(schema_name)
            .or_else(|| crate::commands::command_method(&doc, &[schema_name]))
        {
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

    let method = match find_method(&doc, resource_path, method_name) {
        Ok(method) => method,
        Err(e) => crate::commands::command_method(&doc, &parts[1..]).ok_or(e)?,
    };

    let mut output = build_schema_output(&doc, method);
    if resolve_refs {
        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut output, &doc, &mut seen);
    }
    Ok(output)
}

/// Split a schema path into its service spec and the remaining dotted names.
///
/// An `<api>:<version>` spec keeps a dotted version whole (`dfareporting:v3.4`
/// in `dfareporting:v3.4.ads.list`): the segments after the version that
/// start with a digit continue it, since Discovery resource, method and
/// schema names start with a letter.
fn split_schema_path(path: &str) -> Result<(&str, Vec<&str>), GwsError> {
    let invalid = || {
        GwsError::Validation(format!(
            "Schema path must be 'service.Type' or 'service.resource.method' (e.g. 'drive.files.list', 'youtube:v3.videos.list'), got '{path}'"
        ))
    };
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return Err(invalid());
    }
    let mut spec_parts = 1;
    if parts[0].contains(':') {
        while parts
            .get(spec_parts)
            .is_some_and(|p| p.starts_with(|c: char| c.is_ascii_digit()))
        {
            spec_parts += 1;
        }
    }
    if spec_parts == parts.len() {
        return Err(invalid());
    }
    let spec_len = parts[..spec_parts].iter().map(|p| p.len()).sum::<usize>() + spec_parts - 1;
    Ok((&path[..spec_len], parts[spec_parts..].to_vec()))
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

    // Parameters every method of the API accepts in --params (`fields`,
    // `quotaUser`, `prettyPrint`, ...). A method's own parameter of the same
    // name is the one that applies, so it is listed only under `parameters`.
    let mut api_params = json!({});
    for (name, param) in &doc.parameters {
        if !method.parameters.contains_key(name) {
            api_params[name] = param_to_json(param);
        }
    }
    if api_params.as_object().is_some_and(|m| !m.is_empty()) {
        output["apiParameters"] = api_params;
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

    if let Some(ref r) = schema.schema_ref {
        s["$ref"] = json!(r);
    }
    if !schema.properties.is_empty() {
        s["properties"] = properties_to_json(&schema.properties);
    }
    if !schema.required.is_empty() {
        s["required"] = json!(schema.required);
    }
    if let Some(ref items) = schema.items {
        s["items"] = property_to_json(items);
    }
    if let Some(ref additional) = schema.additional_properties {
        s["additionalProperties"] = property_to_json(additional);
    }

    s
}

fn properties_to_json(properties: &std::collections::HashMap<String, JsonSchemaProperty>) -> Value {
    let mut props = json!({});
    for (name, prop) in properties {
        props[name] = property_to_json(prop);
    }
    props
}

/// One property, including inline object properties, map values
/// (`additionalProperties`) and array items at any depth.
fn property_to_json(prop: &JsonSchemaProperty) -> Value {
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
    if let Some(ref def) = prop.default {
        p["default"] = json!(def);
    }
    if let Some(ref vals) = prop.enum_values {
        p["enum"] = json!(vals);
    }
    if !prop.properties.is_empty() {
        p["properties"] = properties_to_json(&prop.properties);
    }
    if let Some(ref items) = prop.items {
        p["items"] = property_to_json(items);
    }
    if let Some(ref additional) = prop.additional_properties {
        p["additionalProperties"] = property_to_json(additional);
    }
    p
}

/// Recursively resolves "$ref" fields in the JSON value.
///
/// `seen` holds the schemas being expanded on the current branch; a `$ref`
/// to one of them is left unexpanded, which is what ends `$ref` cycles
/// (Docs `Tab.childTabs`, `StructuralElement` → `Table` → … →
/// `StructuralElement`).
fn resolve_schema_refs(
    val: &mut Value,
    doc: &RestDescription,
    seen: &mut std::collections::HashSet<String>,
) {
    match val {
        Value::Object(map) => {
            // Expand a reference not already being expanded on this branch.
            let expanding = map
                .get("$ref")
                .and_then(Value::as_str)
                .filter(|name| !seen.contains(*name))
                .and_then(|name| doc.schemas.get(name).map(|s| (name.to_string(), s)));
            if let Some((name, schema)) = &expanding {
                // Merge the referenced schema, keeping fields the reference
                // already has (usually `$ref` stands alone).
                if let Value::Object(resolved) = schema_to_json(schema) {
                    for (k, v) in resolved {
                        map.entry(k).or_insert(v);
                    }
                }
                seen.insert(name.clone());
            }

            // Walk every field once, with this schema still marked as being
            // expanded, so nested references back to it stay unexpanded.
            for (_, v) in map.iter_mut() {
                resolve_schema_refs(v, doc, seen);
            }

            if let Some((name, _)) = &expanding {
                seen.remove(name);
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

    /// Inline object properties, maps (`additionalProperties`), nested array
    /// items and enums are part of the body the executor validates, so
    /// `gwsr schema` must show them rather than a bare `"type": "object"`.
    #[test]
    fn schema_to_json_keeps_nested_structure() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": {
                "File": {
                    "type": "object",
                    "properties": {
                        "contentHints": {
                            "type": "object",
                            "properties": {
                                "thumbnail": {
                                    "type": "object",
                                    "properties": {
                                        "image": {"type": "string", "format": "byte"},
                                        "owner": {"$ref": "User"}
                                    }
                                }
                            }
                        },
                        "appProperties": {
                            "type": "object",
                            "additionalProperties": {"type": "string"}
                        },
                        "values": {
                            "type": "array",
                            "items": {"type": "array", "items": {"type": "any"}}
                        },
                        "permissions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {"role": {"type": "string"}}
                            }
                        },
                        "status": {"type": "string", "enum": ["ACTIVE", "DELETED"]}
                    }
                },
                "Labels": {
                    "type": "object",
                    "additionalProperties": {"$ref": "User"}
                },
                "User": {
                    "type": "object",
                    "properties": {"email": {"type": "string"}}
                }
            }
        }))
        .unwrap();

        let file = schema_to_json(&doc.schemas["File"]);
        let props = &file["properties"];
        assert_eq!(
            props["contentHints"]["properties"]["thumbnail"]["properties"]["image"]["format"],
            "byte",
            "{file:#}"
        );
        assert_eq!(
            props["appProperties"]["additionalProperties"]["type"], "string",
            "{file:#}"
        );
        assert_eq!(props["values"]["items"]["items"]["type"], "any", "{file:#}");
        assert_eq!(
            props["permissions"]["items"]["properties"]["role"]["type"], "string",
            "{file:#}"
        );
        assert_eq!(props["status"]["enum"], json!(["ACTIVE", "DELETED"]));

        let labels = schema_to_json(&doc.schemas["Labels"]);
        assert_eq!(labels["additionalProperties"]["$ref"], "User", "{labels:#}");

        // `--resolve-refs` follows references inside inline objects too.
        let mut resolved = file.clone();
        resolve_schema_refs(&mut resolved, &doc, &mut std::collections::HashSet::new());
        assert_eq!(
            resolved["properties"]["contentHints"]["properties"]["thumbnail"]["properties"]["owner"]
                ["properties"]["email"]["type"],
            "string",
            "{resolved:#}"
        );
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

    /// Resolve `root` against `schemas` on a worker thread, failing instead
    /// of hanging when resolution does not finish.
    fn resolve_with_deadline(schemas: Value, root: Value) -> Value {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": schemas
        }))
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let mut val = root;
                resolve_schema_refs(&mut val, &doc, &mut std::collections::HashSet::new());
                tx.send(val).unwrap();
            })
            .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("resolve_schema_refs did not finish within 10s")
    }

    #[test]
    fn test_resolve_schema_refs_self_cycle_terminates() {
        // Docs `Tab.childTabs` is an array of `Tab`: a `$ref` cycle.
        let schemas = json!({
            "Tab": { "type": "object", "properties": {
                "title": { "type": "string" },
                "childTabs": { "type": "array", "items": { "$ref": "Tab" } }
            } }
        });
        let out = resolve_with_deadline(schemas, json!({ "response": { "$ref": "Tab" } }));
        let tab = &out["response"];
        assert_eq!(tab["properties"]["title"]["type"], "string");
        // The recursive occurrence stays an unexpanded reference.
        assert_eq!(
            tab["properties"]["childTabs"]["items"],
            json!({ "$ref": "Tab" })
        );
    }

    #[test]
    fn test_resolve_schema_refs_mutual_cycle_terminates() {
        // Docs: StructuralElement -> Table -> TableRow -> TableCell -> StructuralElement.
        let schemas = json!({
            "A": { "type": "object", "properties": { "b": { "$ref": "B" } } },
            "B": { "type": "object", "properties": {
                "c": { "type": "array", "items": { "$ref": "C" } } } },
            "C": { "type": "object", "properties": { "a": { "$ref": "A" } } }
        });
        let out = resolve_with_deadline(schemas, json!({ "request": { "$ref": "A" } }));
        let c = &out["request"]["properties"]["b"]["properties"]["c"]["items"];
        assert_eq!(c["type"], "object");
        assert_eq!(c["properties"]["a"], json!({ "$ref": "A" }));
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

    /// `--params` accepts the API-wide parameters (`fields`, `quotaUser`),
    /// and a rejected one points at `gwsr schema`, so schema lists them.
    #[test]
    fn build_schema_output_lists_api_wide_parameters() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "parameters": {
                "fields": { "type": "string", "location": "query",
                            "description": "Selector specifying a subset of fields." },
                "quotaUser": { "type": "string", "location": "query" },
                "alt": { "type": "string", "location": "query", "default": "json",
                         "enum": ["json", "media"] }
            },
            "resources": { "items": { "methods": {
                "get": { "httpMethod": "GET", "path": "items/{id}",
                    "parameters": {
                        "id": { "type": "string", "location": "path", "required": true },
                        "alt": { "type": "string", "location": "query", "enum": ["json"] }
                    } }
            } } }
        }))
        .unwrap();
        let get = find_method(&doc, &["items"], "get").unwrap();
        let out = build_schema_output(&doc, get);
        assert_eq!(out["apiParameters"]["fields"]["type"], "string");
        assert_eq!(out["apiParameters"]["fields"]["location"], "query");
        assert_eq!(
            out["apiParameters"]["fields"]["description"],
            "Selector specifying a subset of fields."
        );
        assert!(out["apiParameters"].get("quotaUser").is_some());
        // The method's own `alt` wins and is not listed twice.
        assert!(out["apiParameters"].get("alt").is_none());
        assert_eq!(out["parameters"]["alt"]["enum"], json!(["json"]));
        assert!(out["parameters"].get("fields").is_none());

        // An API without global parameters gets no empty object.
        let bare: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "resources": { "items": { "methods": {
                "list": { "httpMethod": "GET", "path": "items" } } } }
        }))
        .unwrap();
        let list = find_method(&bare, &["items"], "list").unwrap();
        assert!(
            build_schema_output(&bare, list)
                .get("apiParameters")
                .is_none()
        );
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

    #[test]
    fn split_schema_path_keeps_dotted_versions_whole() {
        let ok = |p| split_schema_path(p).unwrap();
        assert_eq!(ok("drive.files.list"), ("drive", vec!["files", "list"]));
        assert_eq!(ok("drive:v2.File"), ("drive:v2", vec!["File"]));
        assert_eq!(
            ok("admin:directory_v1.users.list"),
            ("admin:directory_v1", vec!["users", "list"])
        );
        assert_eq!(
            ok("dfareporting:v3.4.ads.list"),
            ("dfareporting:v3.4", vec!["ads", "list"])
        );
        assert_eq!(ok("x:v1.1.2.Type"), ("x:v1.1.2", vec!["Type"]));
        // Without a `:version` there is no version to continue.
        assert_eq!(ok("drive.4.list"), ("drive", vec!["4", "list"]));
        // A version with nothing after it is not a schema path.
        for bad in ["dfareporting:v3.4", "x:v1.1.2", "x:v1."] {
            assert!(
                matches!(split_schema_path(bad), Err(GwsError::Validation(_))),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn test_handle_schema_command_rejects_malformed_paths() {
        for bad in ["drive", "drive.", ".files.list", "drive..list"] {
            assert!(matches!(
                handle_schema_command(bad, None, false).await,
                Err(GwsError::Validation(_))
            ));
        }
        // Unknown services fail before any network access, with a suggestion.
        let err = handle_schema_command("drvie.files.list", None, false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Did you mean 'drive'?"), "{err}");
        // Invalid <api>:<version> identifiers are rejected up front.
        assert!(matches!(
            handle_schema_command("../x:v1.a.b", None, false).await,
            Err(GwsError::Validation(_))
        ));
    }
}
