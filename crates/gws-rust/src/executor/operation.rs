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

//! Long-running operations (`--wait`).
//!
//! Many methods return a `google.longrunning.Operation`
//! (`{"name": ..., "done": false, "metadata": ...}`). With `--wait` the
//! executor polls the API's own `operations.get` until `done` is true, then
//! yields the operation's `response` or turns its `error` into a failure.

use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{Map, Value, json};

use gws_rust_core::client::Idempotency;

use super::errors::error_from_response;
use super::transport::{Transport, read_body};
use super::url::{UrlTarget, build_url};
use crate::discovery::{RestDescription, RestMethod, RestResource};
use crate::error::GwsError;

/// Polling settings for `--wait`.
#[derive(Debug, Clone)]
pub struct WaitConfig {
    /// Give up after this long.
    pub timeout: Duration,
    /// First poll interval; grows by 1.5x per poll up to `max_interval`.
    pub initial_interval: Duration,
    pub max_interval: Duration,
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(600),
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(10),
        }
    }
}

/// Whether `value` looks like a long-running Operation.
pub(crate) fn is_operation(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    obj.get("name").is_some_and(Value::is_string)
        && (obj.contains_key("done")
            || obj.contains_key("metadata")
            || obj
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|k| k.ends_with("#operation")))
}

pub(crate) fn is_done(op: &Value) -> bool {
    op.get("done").and_then(Value::as_bool) == Some(true)
}

/// Find the API's `operations.get` method (top-level `operations` resource
/// preferred, else the shallowest nested one).
pub(crate) fn find_operations_get(doc: &RestDescription) -> Option<&RestMethod> {
    fn search(resources: &std::collections::HashMap<String, RestResource>) -> Option<&RestMethod> {
        if let Some(m) = resources
            .get("operations")
            .and_then(|r| r.methods.get("get"))
        {
            return Some(m);
        }
        let mut names: Vec<&String> = resources.keys().collect();
        names.sort();
        names
            .into_iter()
            .find_map(|n| resources.get(n).and_then(|r| search(&r.resources)))
    }
    search(&doc.resources)
}

/// The value a finished operation stands for: its `response`, or the
/// operation itself when it has none. A failed operation becomes an error.
pub(crate) fn operation_result(op: Value) -> Result<Value, GwsError> {
    if let Some(err) = op.get("error").filter(|e| !e.is_null()) {
        let code = err
            .get("code")
            .and_then(Value::as_u64)
            .and_then(|c| u16::try_from(c).ok())
            .unwrap_or(500);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("long-running operation failed without a message")
            .to_string();
        return Err(GwsError::Api {
            code,
            message: format!(
                "Operation {} failed: {message}",
                op.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("(unnamed)")
            ),
            reason: "operationFailed".to_string(),
            enable_url: None,
        });
    }
    Ok(match op {
        Value::Object(mut map) => match map.remove("response") {
            Some(response) => response,
            None => Value::Object(map),
        },
        other => other,
    })
}

/// Poll `op` until done or the timeout elapses; returns the final operation.
pub(crate) async fn wait(
    transport: &Transport,
    doc: &RestDescription,
    mut op: Value,
    cfg: &WaitConfig,
) -> Result<Value, GwsError> {
    if is_done(&op) {
        return Ok(op);
    }
    let get = find_operations_get(doc).ok_or_else(|| {
        GwsError::Validation(format!(
            "--wait: the {} API has no operations.get method to poll",
            doc.name
        ))
    })?;
    let name = op
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GwsError::other(anyhow::anyhow!("operation has no name to poll")))?
        .to_string();
    let param = get
        .parameter_order
        .first()
        .cloned()
        .unwrap_or_else(|| "name".to_string());
    // With a plain `{name}` segment (drive `operations/{name}`), pass the id,
    // not the full `operations/<id>` resource name.
    let plain_segment = !get.path.contains(&format!("{{+{param}}}"));
    let value = if plain_segment {
        name.rsplit('/').next().unwrap_or(&name).to_string()
    } else {
        name.clone()
    };
    let mut params = Map::new();
    params.insert(param, Value::String(value));
    let url = build_url(doc, get, &params, UrlTarget::Method, transport.endpoints())?;

    let started = Instant::now();
    let mut interval = cfg.initial_interval;
    let auth = transport.auth_method()?;
    while !is_done(&op) {
        if started.elapsed() + interval > cfg.timeout {
            return Err(GwsError::other(anyhow::anyhow!(
                "--wait: operation {name} did not finish within {}s; last state: {}",
                cfg.timeout.as_secs(),
                op.get("metadata").cloned().unwrap_or(json!(null))
            )));
        }
        eprintln!("Waiting for operation {name}...");
        tokio::time::sleep(interval).await;
        interval = interval.mul_f64(1.5).min(cfg.max_interval);

        let sent = transport
            .send(Method::GET, &url.url, Idempotency::Idempotent, |rb| {
                rb.query(&url.query)
            })
            .await?;
        let status = sent.response.status();
        let note = sent.retry_note();
        let body = read_body(sent.response, transport.retry.response_timeout).await?;
        let text = String::from_utf8_lossy(&body);
        if !status.is_success() {
            return Err(error_from_response(status, &text, &auth, note.as_deref()));
        }
        op = serde_json::from_str(&text).map_err(|e| {
            GwsError::other(anyhow::anyhow!("operations.get returned invalid JSON: {e}"))
        })?;
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_detection() {
        assert!(is_operation(
            &json!({"name": "operations/1", "done": false})
        ));
        assert!(is_operation(
            &json!({"name": "x", "kind": "drive#operation"})
        ));
        assert!(!is_operation(&json!({"name": "a file"})));
        assert!(!is_operation(&json!([1])));
    }

    #[test]
    fn result_extraction() {
        assert_eq!(
            operation_result(json!({"name": "o", "done": true, "response": {"ok": 1}})).unwrap(),
            json!({"ok": 1})
        );
        assert_eq!(
            operation_result(json!({"name": "o", "done": true})).unwrap(),
            json!({"name": "o", "done": true})
        );
        let err = operation_result(json!({"name": "o", "done": true,
                                          "error": {"code": 9, "message": "precondition"}}))
        .unwrap_err();
        assert!(err.to_string().contains("precondition"));
    }

    #[test]
    fn finds_operations_get() {
        let mut ops = RestResource::default();
        ops.methods.insert(
            "get".into(),
            RestMethod {
                id: Some("x.operations.get".into()),
                ..Default::default()
            },
        );
        let mut nested = RestResource::default();
        nested.resources.insert("operations".into(), ops);
        let mut doc = RestDescription::default();
        doc.resources.insert("projects".into(), nested);
        assert_eq!(
            find_operations_get(&doc).and_then(|m| m.id.as_deref()),
            Some("x.operations.get")
        );
        assert!(find_operations_get(&RestDescription::default()).is_none());
    }
}
