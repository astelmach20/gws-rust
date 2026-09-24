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

//! Validation of request bodies against Discovery JSON schemas.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::discovery::{JsonSchemaProperty, RestDescription};
use crate::error::GwsError;

/// Human-readable JSON type name.
pub(crate) fn value_type(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number (float)",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

struct Checker<'a> {
    doc: &'a RestDescription,
    allow_unknown_fields: bool,
    errors: Vec<String>,
    unknown: Vec<String>,
}

/// Validate `body` against schema `schema_name`.
///
/// Unknown properties are errors unless `allow_unknown_fields` is set, in
/// which case they are reported on stderr and sent as-is (for Developer
/// Preview fields that are not yet in the published Discovery document).
pub(crate) fn validate_body(
    body: &Value,
    schema_name: &str,
    doc: &RestDescription,
    allow_unknown_fields: bool,
) -> Result<(), GwsError> {
    let mut checker = Checker {
        doc,
        allow_unknown_fields,
        errors: Vec::new(),
        unknown: Vec::new(),
    };
    checker.value(body, schema_name, "$");

    if !checker.unknown.is_empty() {
        eprintln!(
            "warning: sending fields not in the Discovery schema (--allow-unknown-fields): {}",
            checker.unknown.join(", ")
        );
    }
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(GwsError::Validation(format!(
            "Request body failed schema validation:\n- {}",
            checker.errors.join("\n- ")
        )))
    }
}

impl Checker<'_> {
    fn value(&mut self, value: &Value, schema_ref_name: &str, path: &str) {
        let Some(schema) = self.doc.schemas.get(schema_ref_name) else {
            self.errors
                .push(format!("{path}: Schema '{schema_ref_name}' not found"));
            return;
        };
        if schema.schema_type.as_deref() == Some("object") || !schema.properties.is_empty() {
            match value {
                Value::Object(obj) => {
                    self.properties(obj, &schema.properties, &schema.required, path);
                }
                _ => self.errors.push(format!("{path}: Expected object")),
            }
        }
    }

    fn properties(
        &mut self,
        obj: &Map<String, Value>,
        properties: &HashMap<String, JsonSchemaProperty>,
        required_keys: &[String],
        path: &str,
    ) {
        for req_key in required_keys {
            if !obj.contains_key(req_key) {
                self.errors
                    .push(format!("{path}: Missing required property '{req_key}'"));
            }
        }
        for (key, val) in obj {
            let current_path = if path == "$" {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            match properties.get(key) {
                Some(prop) => self.property(val, prop, &current_path),
                None if self.allow_unknown_fields => self.unknown.push(current_path),
                None => {
                    let mut valid: Vec<&str> = properties.keys().map(String::as_str).collect();
                    valid.sort_unstable();
                    let hint = super::input::closest(key, valid.iter().copied())
                        .map(|b| format!(" Did you mean '{b}'?"))
                        .unwrap_or_default();
                    self.errors.push(format!(
                        "{current_path}: Unknown property.{hint} Valid properties: {valid:?} \
                         (use --allow-unknown-fields to send fields missing from Discovery)"
                    ));
                }
            }
        }
    }

    fn property(&mut self, value: &Value, prop: &JsonSchemaProperty, path: &str) {
        if let Some(ref_name) = &prop.schema_ref {
            self.value(value, ref_name, path);
            return;
        }

        if let Some(expected) = &prop.prop_type {
            let type_matches = match (expected.as_str(), value) {
                ("string", Value::String(_)) => true,
                ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
                ("number", Value::Number(_)) => true,
                ("boolean", Value::Bool(_)) => true,
                ("array", Value::Array(_)) => true,
                ("object", Value::Object(_)) => true,
                ("any", _) => true,
                _ => false,
            };
            if !type_matches {
                self.errors.push(format!(
                    "{path}: Expected type '{expected}', found {}",
                    value_type(value)
                ));
                return;
            }
        }

        if prop.prop_type.as_deref() == Some("array")
            && let Some(items) = &prop.items
            && let Value::Array(arr) = value
        {
            for (i, item) in arr.iter().enumerate() {
                self.property(item, items, &format!("{path}[{i}]"));
            }
        }

        if prop.prop_type.as_deref() == Some("object")
            && let Value::Object(obj) = value
        {
            if !prop.properties.is_empty() {
                self.properties(obj, &prop.properties, &[], path);
            } else if let Some(additional) = &prop.additional_properties {
                for (k, v) in obj {
                    self.property(v, additional, &format!("{path}.{k}"));
                }
            }
        }

        if let Some(enum_values) = &prop.enum_values
            && let Value::String(s) = value
            && !enum_values.contains(s)
        {
            self.errors.push(format!(
                "{path}: Value '{s}' is not a valid enum member. Valid options: {enum_values:?}"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::JsonSchema;
    use serde_json::json;

    fn string_prop() -> JsonSchemaProperty {
        JsonSchemaProperty {
            prop_type: Some("string".to_string()),
            ..Default::default()
        }
    }

    fn doc() -> RestDescription {
        let mut properties = HashMap::new();
        properties.insert("name".to_string(), string_prop());
        properties.insert(
            "status".to_string(),
            JsonSchemaProperty {
                enum_values: Some(vec!["ACTIVE".into(), "INACTIVE".into()]),
                ..string_prop()
            },
        );
        properties.insert(
            "count".to_string(),
            JsonSchemaProperty {
                prop_type: Some("integer".into()),
                ..Default::default()
            },
        );
        properties.insert(
            "tags".to_string(),
            JsonSchemaProperty {
                prop_type: Some("array".into()),
                items: Some(Box::new(string_prop())),
                ..Default::default()
            },
        );
        properties.insert(
            "labels".to_string(),
            JsonSchemaProperty {
                prop_type: Some("object".into()),
                additional_properties: Some(Box::new(string_prop())),
                ..Default::default()
            },
        );
        properties.insert(
            "parent".to_string(),
            JsonSchemaProperty {
                schema_ref: Some("Parent".into()),
                ..Default::default()
            },
        );
        let mut parent_props = HashMap::new();
        parent_props.insert("id".to_string(), string_prop());

        let mut schemas = HashMap::new();
        schemas.insert(
            "File".to_string(),
            JsonSchema {
                schema_type: Some("object".into()),
                required: vec!["name".into()],
                properties,
                ..Default::default()
            },
        );
        schemas.insert(
            "Parent".to_string(),
            JsonSchema {
                schema_type: Some("object".into()),
                properties: parent_props,
                ..Default::default()
            },
        );
        RestDescription {
            schemas,
            ..Default::default()
        }
    }

    fn check(body: Value, allow_unknown: bool) -> Result<(), GwsError> {
        validate_body(&body, "File", &doc(), allow_unknown)
    }

    #[test]
    fn valid_body() {
        check(
            json!({"name": "f", "status": "ACTIVE", "count": 1, "tags": ["a"],
                   "labels": {"k": "v"}, "parent": {"id": "1"}}),
            false,
        )
        .unwrap();
    }

    #[test]
    fn errors_are_reported() {
        let cases = [
            (
                json!({"status": "ACTIVE"}),
                "Missing required property 'name'",
            ),
            (
                json!({"name": "f", "status": "UNKNOWN"}),
                "not a valid enum member",
            ),
            (
                json!({"name": "f", "count": "1"}),
                "Expected type 'integer', found string",
            ),
            (
                json!({"name": "f", "parent": {"bad": 1}}),
                "Unknown property",
            ),
            (
                json!({"name": "f", "labels": {"k": 1}}),
                "Expected type 'string'",
            ),
            (json!([]), "Expected object"),
        ];
        for (body, needle) in cases {
            let msg = check(body.clone(), false).unwrap_err().to_string();
            assert!(msg.contains(needle), "{body}: {msg}");
        }
    }

    #[test]
    fn unknown_field_suggestion_and_escape_hatch() {
        let msg = check(json!({"name": "f", "nmae": "x"}), false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("Did you mean 'name'"), "{msg}");
        check(json!({"name": "f", "previewField": 1}), true).unwrap();
        // Other errors still fail with the escape hatch.
        assert!(check(json!({"name": 1, "previewField": 1}), true).is_err());
    }

    #[test]
    fn value_type_names() {
        assert_eq!(value_type(&json!(null)), "null");
        assert_eq!(value_type(&json!(true)), "boolean");
        assert_eq!(value_type(&json!(42)), "integer");
        assert_eq!(value_type(&json!(3.5)), "number (float)");
        assert_eq!(value_type(&json!("s")), "string");
        assert_eq!(value_type(&json!([1])), "array");
        assert_eq!(value_type(&json!({"a": 1})), "object");
    }
}
