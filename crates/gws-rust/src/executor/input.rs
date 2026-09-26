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

//! Reading and validating `--params` / `--json` input against Discovery.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use tokio::io::AsyncReadExt;

use crate::discovery::{MethodParameter, RestDescription, RestMethod};
use crate::error::GwsError;

/// Where a JSON flag value comes from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JsonArg<'a> {
    Inline(&'a str),
    File(&'a str),
    Stdin,
}

impl<'a> JsonArg<'a> {
    /// `@path` reads a file, `-` reads stdin, anything else is inline JSON.
    pub(crate) fn parse(raw: &'a str) -> Self {
        if raw == "-" {
            JsonArg::Stdin
        } else if let Some(path) = raw.strip_prefix('@') {
            JsonArg::File(path)
        } else {
            JsonArg::Inline(raw)
        }
    }
}

/// Resolve `--params` and `--json` values to JSON text. At most one of them
/// may read stdin.
pub(crate) async fn read_json_args(
    params: Option<&str>,
    body: Option<&str>,
) -> Result<(Option<String>, Option<String>), GwsError> {
    let params = params.map(JsonArg::parse);
    let body = body.map(JsonArg::parse);
    if params == Some(JsonArg::Stdin) && body == Some(JsonArg::Stdin) {
        return Err(GwsError::Validation(
            "--params - and --json - cannot both read stdin; put one of them in a file and use @path"
                .to_string(),
        ));
    }
    let params = match params {
        Some(arg) => Some(resolve(arg, "--params").await?),
        None => None,
    };
    let body = match body {
        Some(arg) => Some(resolve(arg, "--json").await?),
        None => None,
    };
    Ok((params, body))
}

async fn resolve(arg: JsonArg<'_>, flag: &str) -> Result<String, GwsError> {
    match arg {
        JsonArg::Inline(s) => Ok(s.to_string()),
        JsonArg::File(path) => {
            let safe = crate::validate::validate_safe_file_path(path, flag)?;
            tokio::fs::read_to_string(&safe)
                .await
                .map_err(|e| GwsError::Validation(format!("{flag}: failed to read '{path}': {e}")))
        }
        JsonArg::Stdin => {
            let mut buf = String::new();
            tokio::io::stdin()
                .read_to_string(&mut buf)
                .await
                .map_err(|e| GwsError::Validation(format!("{flag}: failed to read stdin: {e}")))?;
            Ok(buf)
        }
    }
}

/// Parse `--params` text into a JSON object.
pub(crate) fn parse_params(text: Option<&str>) -> Result<Map<String, Value>, GwsError> {
    let Some(text) = text else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(GwsError::Validation(format!(
            "--params must be a JSON object, got {}",
            super::body_schema::value_type(&other)
        ))),
        Err(e) => Err(GwsError::Validation(format!("Invalid --params JSON: {e}"))),
    }
}

/// Parse `--json` text.
pub(crate) fn parse_body(text: Option<&str>) -> Result<Option<Value>, GwsError> {
    text.map(|t| {
        serde_json::from_str(t)
            .map_err(|e| GwsError::Validation(format!("Invalid --json body: {e}")))
    })
    .transpose()
}

/// Closest candidate to `name` for "did you mean" hints.
pub(crate) fn closest<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    candidates
        .map(|c| {
            (
                c,
                strsim::damerau_levenshtein(&lower, &c.to_ascii_lowercase()),
            )
        })
        .filter(|(c, d)| *d <= (c.len().max(name.len()) / 3).max(2))
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

fn lookup<'a>(
    doc: &'a RestDescription,
    method: &'a RestMethod,
    name: &str,
) -> Option<&'a MethodParameter> {
    method
        .parameters
        .get(name)
        .or_else(|| doc.parameters.get(name))
}

/// Validate `--params` against the method's (and the API's global)
/// parameters: names, types, enums, repetition and required-ness.
///
/// With `allow_unknown`, parameters not in Discovery are passed through as
/// query parameters with a warning instead of being rejected.
pub(crate) fn validate_params(
    doc: &RestDescription,
    method: &RestMethod,
    params: &Map<String, Value>,
    allow_unknown: bool,
) -> Result<(), GwsError> {
    let mut errors = Vec::new();
    let request_fields: BTreeSet<&str> = method
        .request
        .as_ref()
        .and_then(|r| r.schema_ref.as_deref())
        .and_then(|name| doc.schemas.get(name))
        .map(|s| s.properties.keys().map(String::as_str).collect())
        .unwrap_or_default();

    for (name, value) in params {
        let Some(def) = lookup(doc, method, name) else {
            if allow_unknown {
                tracing::warn!(
                    "sending unknown parameter '{name}' as a query parameter (--allow-unknown-params)"
                );
                continue;
            }
            let candidates = method.parameters.keys().chain(doc.parameters.keys());
            let mut msg = format!("unknown parameter '{name}'");
            if let Some(best) = closest(name, candidates.map(String::as_str)) {
                msg.push_str(&format!(" (did you mean '{best}'?)"));
            }
            if request_fields.contains(name.as_str()) {
                msg.push_str("; it is a request body field, pass it in --json");
            }
            errors.push(msg);
            continue;
        };
        if def.deprecated {
            tracing::warn!("parameter '{name}' is deprecated");
        }
        check_param_value(name, def, value, &mut errors);
    }

    for (name, def) in &method.parameters {
        if def.required && !params.contains_key(name) {
            let kind = if def.location.as_deref() == Some("path") {
                "path parameter"
            } else {
                "parameter"
            };
            errors.push(format!(
                "required {kind} '{name}' is missing; provide it via --params"
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        errors.sort();
        Err(GwsError::Validation(format!(
            "Invalid --params for {}:\n- {}\nRun `gwsr schema {}` to see the accepted parameters.",
            method.id.as_deref().unwrap_or("this method"),
            errors.join("\n- "),
            method.id.as_deref().unwrap_or("<service.resource.method>"),
        )))
    }
}

fn check_param_value(name: &str, def: &MethodParameter, value: &Value, errors: &mut Vec<String>) {
    match value {
        Value::Array(items) if def.repeated => {
            for (i, item) in items.iter().enumerate() {
                check_scalar(&format!("{name}[{i}]"), def, item, errors);
            }
        }
        Value::Array(_) => errors.push(format!(
            "parameter '{name}' is not repeated and cannot take an array"
        )),
        other => check_scalar(name, def, other, errors),
    }
}

fn check_scalar(name: &str, def: &MethodParameter, value: &Value, errors: &mut Vec<String>) {
    let ty = def.param_type.as_deref().unwrap_or("string");
    let ok = match (ty, value) {
        (_, Value::Null | Value::Object(_) | Value::Array(_)) => false,
        ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
        ("integer", Value::String(s)) => s.parse::<i64>().is_ok() || s.parse::<u64>().is_ok(),
        ("integer", _) => false,
        ("number", Value::Number(_)) => true,
        ("number", Value::String(s)) => s.parse::<f64>().is_ok(),
        ("number", _) => false,
        ("boolean", Value::Bool(_)) => true,
        ("boolean", Value::String(s)) => s == "true" || s == "false",
        ("boolean", _) => false,
        _ => true,
    };
    if !ok {
        errors.push(format!(
            "parameter '{name}' expects {ty}, got {} {}",
            super::body_schema::value_type(value),
            value
        ));
        return;
    }
    if let (Some(allowed), Value::String(s)) = (&def.enum_values, value)
        && !allowed.iter().any(|a| a == s)
    {
        let hint = closest(s, allowed.iter().map(String::as_str))
            .map(|b| format!(" (did you mean '{b}'?)"))
            .unwrap_or_default();
        errors.push(format!(
            "parameter '{name}' has invalid value '{s}'{hint}; allowed: {}",
            allowed.join(", ")
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn param(ty: &str) -> MethodParameter {
        MethodParameter {
            param_type: Some(ty.to_string()),
            location: Some("query".to_string()),
            ..Default::default()
        }
    }

    fn doc_and_method() -> (RestDescription, RestMethod) {
        let mut parameters = HashMap::new();
        parameters.insert("pageSize".to_string(), param("integer"));
        parameters.insert("q".to_string(), param("string"));
        parameters.insert("includeLabels".to_string(), param("boolean"));
        parameters.insert(
            "corpora".to_string(),
            MethodParameter {
                enum_values: Some(vec!["user".into(), "domain".into(), "drive".into()]),
                ..param("string")
            },
        );
        parameters.insert(
            "ids".to_string(),
            MethodParameter {
                repeated: true,
                ..param("string")
            },
        );
        parameters.insert(
            "fileId".to_string(),
            MethodParameter {
                location: Some("path".into()),
                required: true,
                ..param("string")
            },
        );
        let mut global = HashMap::new();
        global.insert("fields".to_string(), param("string"));
        let doc = RestDescription {
            parameters: global,
            ..Default::default()
        };
        let method = RestMethod {
            id: Some("drive.files.list".into()),
            parameters,
            ..Default::default()
        };
        (doc, method)
    }

    fn validate(params: Value, allow_unknown: bool) -> Result<(), GwsError> {
        let (doc, method) = doc_and_method();
        let Value::Object(map) = params else {
            unreachable!()
        };
        validate_params(&doc, &method, &map, allow_unknown)
    }

    #[test]
    fn json_arg_forms() {
        assert_eq!(JsonArg::parse("-"), JsonArg::Stdin);
        assert_eq!(JsonArg::parse("@body.json"), JsonArg::File("body.json"));
        assert_eq!(JsonArg::parse("{\"a\":1}"), JsonArg::Inline("{\"a\":1}"));
    }

    #[tokio::test]
    async fn both_stdin_is_rejected() {
        let err = read_json_args(Some("-"), Some("-")).await.unwrap_err();
        assert!(err.to_string().contains("cannot both read stdin"));
    }

    // Uses an absolute path: changing the process working directory would
    // leak into every test running concurrently (and into their children).
    #[tokio::test]
    async fn reads_json_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("body.json");
        std::fs::write(&file, r#"{"name":"x"}"#).unwrap();
        let arg = format!("@{}", file.display());
        let (params, body) = read_json_args(Some(r#"{"a":1}"#), Some(&arg))
            .await
            .unwrap();
        assert_eq!(params.as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(body.as_deref(), Some(r#"{"name":"x"}"#));
    }

    #[test]
    fn params_must_be_object() {
        assert!(parse_params(Some("[1]")).is_err());
        assert!(parse_params(Some("{")).is_err());
        assert_eq!(parse_params(None).unwrap().len(), 0);
    }

    #[test]
    fn valid_params_pass() {
        validate(
            json!({"fileId": "f", "pageSize": 5, "q": "x", "includeLabels": "true",
                   "corpora": "user", "ids": ["a", "b"], "fields": "files(id)"}),
            false,
        )
        .unwrap();
        // int64-as-string is accepted for integers
        validate(json!({"fileId": "f", "pageSize": "10"}), false).unwrap();
    }

    #[test]
    fn unknown_param_suggests_closest() {
        let err = validate(json!({"fileId": "f", "pageSze": 5}), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown parameter 'pageSze'"), "{msg}");
        assert!(msg.contains("did you mean 'pageSize'"), "{msg}");
    }

    #[test]
    fn unknown_param_allowed_with_escape_hatch() {
        validate(json!({"fileId": "f", "newPreviewParam": 1}), true).unwrap();
    }

    #[test]
    fn type_and_enum_errors() {
        let msg = validate(json!({"fileId": "f", "pageSize": "abc"}), false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("'pageSize' expects integer"), "{msg}");

        let msg = validate(json!({"fileId": "f", "corpora": "usr"}), false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("invalid value 'usr'"), "{msg}");
        assert!(msg.contains("did you mean 'user'"), "{msg}");

        let msg = validate(json!({"fileId": "f", "q": ["a"]}), false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("not repeated"), "{msg}");
    }

    #[test]
    fn missing_required() {
        let msg = validate(json!({}), false).unwrap_err().to_string();
        assert!(
            msg.contains("required path parameter 'fileId' is missing"),
            "{msg}"
        );
    }

    #[test]
    fn closest_matches() {
        let c = ["pageSize", "pageToken", "q"];
        assert_eq!(closest("pagesize", c.iter().copied()), Some("pageSize"));
        assert_eq!(closest("zzzzzzzz", c.iter().copied()), None);
    }
}
