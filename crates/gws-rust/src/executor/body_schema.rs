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

/// Whether `n` is a valid JSON number for a string property of Discovery
/// `format` `int64` or `uint64`: an integer in that format's range.
fn int64_number(format: Option<&str>, n: &serde_json::Number) -> bool {
    match format {
        Some("int64") => n.is_i64(),
        Some("uint64") => n.is_u64(),
        _ => false,
    }
}

/// Whether `s` is a valid value for a string property of Discovery
/// `format`: for `int64` and `uint64`, a decimal integer in that format's
/// range (the same check as [`int64_number`] and [`integer_string`], so
/// `"1.5"` is refused); any string for every other format.
fn string_value(format: Option<&str>, s: &str) -> bool {
    match format {
        Some("int64") => s.parse::<i64>().is_ok(),
        Some("uint64") => s.parse::<u64>().is_ok(),
        _ => true,
    }
}

/// Whether `v` is in range for an integer property of Discovery `format`
/// `int32`, `uint32` (or no format: any 64-bit integer, signed or unsigned).
fn integer_in_range(format: Option<&str>, v: i128) -> bool {
    match format {
        Some("int32") => i32::try_from(v).is_ok(),
        Some("uint32") => u32::try_from(v).is_ok(),
        _ => i64::try_from(v).is_ok() || u64::try_from(v).is_ok(),
    }
}

/// Whether `n` is a valid JSON number for an integer property: a whole
/// number (`1.0` is a float, refused) in the range of its `format`.
fn integer_number(format: Option<&str>, n: &serde_json::Number) -> bool {
    n.as_i64()
        .map(i128::from)
        .or_else(|| n.as_u64().map(i128::from))
        .is_some_and(|v| integer_in_range(format, v))
}

/// Whether `s` is a valid proto3 JSON string form of an integer property,
/// with the same range check as [`integer_number`] (`"1.0"` is refused).
fn integer_string(format: Option<&str>, s: &str) -> bool {
    s.parse::<i128>().is_ok_and(|v| integer_in_range(format, v))
}

/// Whether the finite `v` is in range for a number property of Discovery
/// `format`: a `float` is a 32-bit float, so a value that rounds beyond
/// `f32::MAX` is refused (the API's proto3 JSON parser rejects it); `double`
/// (or no format) takes any finite f64.
fn number_in_range(format: Option<&str>, v: f64) -> bool {
    match format {
        // Rounding to the nearest f32 overflows to infinity exactly when
        // `v` is beyond the float range.
        Some("float") => (v as f32).is_finite(),
        _ => true,
    }
}

/// Whether `n` is a valid JSON number for a number property of `format`.
fn number_number(format: Option<&str>, n: &serde_json::Number) -> bool {
    n.as_f64()
        .is_some_and(|v| v.is_finite() && number_in_range(format, v))
}

/// Whether `s` is a valid proto3 JSON string form of a `double` or `float`:
/// a finite decimal number in the format's range, `"NaN"`, `"Infinity"` or
/// `"-Infinity"`.
fn number_string(format: Option<&str>, s: &str) -> bool {
    matches!(s, "NaN" | "Infinity" | "-Infinity")
        || (s.trim() == s
            && s.parse::<f64>()
                .is_ok_and(|v| v.is_finite() && number_in_range(format, v)))
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
        tracing::warn!(
            "sending fields not in the Discovery schema (--allow-unknown-fields): {}",
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
            // A map schema (e.g. Admin Directory `UserCustomProperties`):
            // its keys are chosen by the caller, only values are typed.
            let map_values = schema
                .additional_properties
                .as_deref()
                .filter(|_| schema.properties.is_empty());
            match (value, map_values) {
                (Value::Object(obj), Some(additional)) => {
                    for (k, v) in obj {
                        let current_path = if path == "$" {
                            k.clone()
                        } else {
                            format!("{path}.{k}")
                        };
                        self.field(v, additional, &current_path);
                    }
                }
                (Value::Object(obj), None) => {
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
                Some(prop) => self.field(val, prop, &current_path),
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

    /// An object field or map entry: `null` clears it whatever its type
    /// (patch semantics; Drive clears `appProperties` entries set to null).
    fn field(&mut self, value: &Value, prop: &JsonSchemaProperty, path: &str) {
        if !value.is_null() {
            self.property(value, prop, path);
        }
    }

    fn property(&mut self, value: &Value, prop: &JsonSchemaProperty, path: &str) {
        if let Some(ref_name) = &prop.schema_ref {
            self.value(value, ref_name, path);
            return;
        }

        if let Some(expected) = &prop.prop_type {
            let type_matches = match (expected.as_str(), value) {
                ("string", Value::String(s)) => string_value(prop.format.as_deref(), s),
                // Discovery types 64-bit integers as strings; the API's JSON
                // parser takes them as numbers too (proto3 JSON mapping).
                ("string", Value::Number(n)) => int64_number(prop.format.as_deref(), n),
                ("integer", Value::Number(n)) => integer_number(prop.format.as_deref(), n),
                // Proto3 JSON also takes integers and floating-point numbers
                // as strings ("5", "1.5", "NaN", "Infinity").
                ("integer", Value::String(s)) => integer_string(prop.format.as_deref(), s),
                ("number", Value::Number(n)) => number_number(prop.format.as_deref(), n),
                ("number", Value::String(s)) => number_string(prop.format.as_deref(), s),
                ("boolean", Value::Bool(_)) => true,
                ("array", Value::Array(_)) => true,
                ("object", Value::Object(_)) => true,
                ("any", _) => true,
                _ => false,
            };
            if !type_matches {
                // The format says why a value of the right JSON type (an
                // `int64` string such as "abc") is refused.
                let format = prop
                    .format
                    .as_deref()
                    .map(|f| format!(" with format '{f}'"))
                    .unwrap_or_default();
                self.errors.push(format!(
                    "{path}: Expected type '{expected}'{format}, found {}",
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
                    self.field(v, additional, &format!("{path}.{k}"));
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
                json!({"name": "f", "count": "one"}),
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

    /// `null` clears a field or map entry (Drive: "Entries with null values
    /// are cleared in update and copy requests" for `appProperties`;
    /// patch semantics in general), so it is valid for any field type.
    #[test]
    fn null_clears_fields_and_map_entries() {
        check(
            json!({"name": "f", "status": null, "count": null, "tags": null,
                   "labels": {"stale": null, "k": "v"}, "parent": null}),
            false,
        )
        .unwrap();
        // The body itself and array elements are not clearable fields.
        for body in [json!(null), json!({"name": "f", "tags": [null]})] {
            assert!(check(body.clone(), false).is_err(), "{body}");
        }
    }

    /// A named schema that is a map (`additionalProperties`, no
    /// `properties`), like Admin Directory `UserCustomProperties`, which
    /// `User.customSchemas` maps to: its keys are user-chosen, not unknown.
    #[test]
    fn named_map_schema_accepts_arbitrary_keys() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "admin", "version": "directory_v1", "rootUrl": "https://admin.googleapis.com/",
            "schemas": {
                "User": { "id": "User", "type": "object", "properties": {
                    "customSchemas": { "type": "object",
                        "additionalProperties": { "$ref": "UserCustomProperties" } }
                } },
                "UserCustomProperties": { "id": "UserCustomProperties", "type": "object",
                    "additionalProperties": { "type": "any" } },
                "Counts": { "id": "Counts", "type": "object",
                    "additionalProperties": { "type": "integer" } }
            }
        }))
        .unwrap();
        validate_body(
            &json!({"customSchemas": {"Employment": {"costCenter": "42", "level": 3}}}),
            "User",
            &doc,
            false,
        )
        .unwrap();
        validate_body(&json!({"a": 1, "b": 2}), "Counts", &doc, false).unwrap();
        // A null entry clears it, as for any map.
        validate_body(&json!({"a": null}), "Counts", &doc, false).unwrap();
        // Map values are still checked against additionalProperties.
        let msg = validate_body(&json!({"a": "x"}), "Counts", &doc, false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("a: Expected type 'integer'"), "{msg}");
    }

    /// Discovery declares 64-bit integers `{"type": "string", "format":
    /// "int64"}`; the API accepts them as JSON numbers as well as strings.
    #[test]
    fn int64_string_fields_accept_integer_numbers() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": { "Req": { "id": "Req", "type": "object", "properties": {
                "size": { "type": "string", "format": "int64" },
                "historyId": { "type": "string", "format": "uint64" },
                "ids": { "type": "array",
                         "items": { "type": "string", "format": "int64" } },
                "name": { "type": "string" }
            } } }
        }))
        .unwrap();
        let check = |body: Value| validate_body(&body, "Req", &doc, false);
        check(json!({"size": "123", "historyId": "9"})).unwrap();
        check(json!({"size": -5, "historyId": 18446744073709551615u64, "ids": [1, "2"]})).unwrap();
        for (body, needle) in [
            (json!({"size": 1.5}), "size: Expected type 'string'"),
            (
                json!({"historyId": -1}),
                "historyId: Expected type 'string'",
            ),
            (
                json!({"size": 18446744073709551615u64}),
                "size: Expected type 'string'",
            ),
            (json!({"name": 7}), "name: Expected type 'string'"),
        ] {
            let msg = check(body.clone()).unwrap_err().to_string();
            assert!(msg.contains(needle), "{body}: {msg}");
        }
    }

    /// A string sent for an `int64`/`uint64` field must be a decimal integer
    /// in that format's range, as the JSON number form and the string form
    /// of an `int32` field must: `"abc"` or `"1.5"` would be refused by the
    /// API after the request was sent.
    #[test]
    fn int64_string_values_are_checked_like_numbers() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": { "Req": { "id": "Req", "type": "object", "properties": {
                "size": { "type": "string", "format": "int64" },
                "historyId": { "type": "string", "format": "uint64" },
                "ids": { "type": "array",
                         "items": { "type": "string", "format": "int64" } },
                "when": { "type": "string", "format": "date-time" },
                "name": { "type": "string" }
            } } }
        }))
        .unwrap();
        let check = |body: Value| validate_body(&body, "Req", &doc, false);
        for body in [
            json!({"size": "-9223372036854775808", "historyId": "18446744073709551615"}),
            json!({"size": "9223372036854775807", "historyId": "0", "ids": ["1", 2]}),
            // Other string formats and plain strings are not number-checked.
            json!({"when": "2026-01-01T00:00:00Z", "name": "1.5"}),
        ] {
            check(body.clone()).unwrap_or_else(|e| panic!("{body}: {e}"));
        }
        for (body, needle) in [
            (json!({"size": "abc"}), "size: "),
            (json!({"size": ""}), "size: "),
            (json!({"size": "1.5"}), "size: "),
            (json!({"size": " 5"}), "size: "),
            (json!({"size": "9223372036854775808"}), "size: "),
            (json!({"historyId": "-1"}), "historyId: "),
            (json!({"historyId": "18446744073709551616"}), "historyId: "),
            (json!({"ids": ["1", "x"]}), "ids[1]: "),
        ] {
            let msg = check(body.clone()).unwrap_err().to_string();
            assert!(msg.contains(needle), "{body}: {msg}");
            assert!(
                msg.contains("format 'int64'") || msg.contains("format 'uint64'"),
                "{body}: {msg}"
            );
        }
    }

    /// The proto3 JSON mapping that Google APIs parse accepts 32-bit integers
    /// and floating-point numbers as strings too, including the special
    /// values `"NaN"`, `"Infinity"` and `"-Infinity"`.
    #[test]
    fn integer_and_number_fields_accept_proto3_json_strings() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": { "Req": { "id": "Req", "type": "object", "properties": {
                "count": { "type": "integer", "format": "int32" },
                "size": { "type": "integer", "format": "uint32" },
                "plain": { "type": "integer" },
                "ratio": { "type": "number", "format": "double" },
                "scale": { "type": "number", "format": "float" },
                "scores": { "type": "array",
                            "items": { "type": "number", "format": "double" } }
            } } }
        }))
        .unwrap();
        let check = |body: Value| validate_body(&body, "Req", &doc, false);
        check(json!({"count": "-5", "size": "4294967295", "plain": "9007199254740993"})).unwrap();
        check(json!({"ratio": "1.5", "scale": "-2e3", "scores": ["NaN", "Infinity", "-Infinity", 0.5]}))
            .unwrap();
        for (body, needle) in [
            (json!({"count": "abc"}), "count: Expected type 'integer'"),
            (json!({"count": ""}), "count: Expected type 'integer'"),
            (json!({"count": "1.5"}), "count: Expected type 'integer'"),
            (
                json!({"count": "2147483648"}),
                "count: Expected type 'integer'",
            ),
            (json!({"size": "-1"}), "size: Expected type 'integer'"),
            (
                json!({"size": "4294967296"}),
                "size: Expected type 'integer'",
            ),
            (json!({"ratio": "nan"}), "ratio: Expected type 'number'"),
            (json!({"ratio": "inf"}), "ratio: Expected type 'number'"),
            (json!({"ratio": "1e400"}), "ratio: Expected type 'number'"),
            (json!({"ratio": " 1.5"}), "ratio: Expected type 'number'"),
            (json!({"ratio": "x"}), "ratio: Expected type 'number'"),
        ] {
            let msg = check(body.clone()).unwrap_err().to_string();
            assert!(msg.contains(needle), "{body}: {msg}");
        }
    }

    /// An `int32`/`uint32` field gets the same range check whether its value
    /// is a JSON number or its proto3 JSON string form, and a fraction such as
    /// `1.0` is refused in both forms.
    #[test]
    fn int32_and_uint32_numbers_are_range_checked_like_strings() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": { "Req": { "id": "Req", "type": "object", "properties": {
                "count": { "type": "integer", "format": "int32" },
                "size": { "type": "integer", "format": "uint32" },
                "plain": { "type": "integer" }
            } } }
        }))
        .unwrap();
        let check = |body: Value| validate_body(&body, "Req", &doc, false);
        for body in [
            json!({"count": 2147483647, "size": 4294967295u64}),
            json!({"count": -2147483648, "size": 0}),
            json!({"count": "2147483647", "size": "4294967295"}),
            json!({"plain": 9223372036854775807i64}),
            json!({"plain": 18446744073709551615u64}),
            json!({"plain": -9223372036854775808i64}),
        ] {
            check(body.clone()).unwrap_or_else(|e| panic!("{body}: {e}"));
        }
        for body in [
            json!({"count": 5000000000i64}),
            json!({"count": "5000000000"}),
            json!({"count": 2147483648i64}),
            json!({"count": -2147483649i64}),
            json!({"count": 1.0}),
            json!({"count": "1.0"}),
            json!({"size": -1}),
            json!({"size": "-1"}),
            json!({"size": 4294967296u64}),
            json!({"size": "4294967296"}),
            json!({"size": 1.0}),
            json!({"plain": 1.0}),
        ] {
            let msg = check(body.clone()).unwrap_err().to_string();
            assert!(msg.contains("Expected type 'integer'"), "{body}: {msg}");
        }
    }

    /// A `float` field holds a 32-bit float: a value beyond its range is
    /// refused by the API's proto3 JSON parser, whether it is a JSON number
    /// or its string form, while a `double` field takes the full f64 range.
    #[test]
    fn float_values_are_range_checked() {
        let doc: RestDescription = serde_json::from_value(json!({
            "name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/",
            "schemas": { "Req": { "id": "Req", "type": "object", "properties": {
                "scale": { "type": "number", "format": "float" },
                "ratio": { "type": "number", "format": "double" },
                "plain": { "type": "number" }
            } } }
        }))
        .unwrap();
        let check = |body: Value| validate_body(&body, "Req", &doc, false);
        for body in [
            json!({"scale": 3.4e38, "ratio": 1e300, "plain": 1e300}),
            json!({"scale": -3.4e38, "ratio": "1e300", "plain": "-1e300"}),
            json!({"scale": "3.4e38"}),
            json!({"scale": 1e-50}),
            json!({"scale": "NaN"}),
            json!({"scale": "-Infinity"}),
        ] {
            check(body.clone()).unwrap_or_else(|e| panic!("{body}: {e}"));
        }
        for body in [
            json!({"scale": 1e39}),
            json!({"scale": -1e39}),
            json!({"scale": "1e39"}),
            json!({"scale": "-3.5e38"}),
        ] {
            let msg = check(body.clone()).unwrap_err().to_string();
            assert!(
                msg.contains("scale: Expected type 'number'"),
                "{body}: {msg}"
            );
        }
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
