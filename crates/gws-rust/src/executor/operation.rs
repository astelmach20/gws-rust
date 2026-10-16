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

use super::url::{UrlTarget, build_url, extract_template_path_parameters, path_template};
use crate::discovery::{RestDescription, RestMethod, RestResource};
use crate::error::GwsError;
use crate::transport::errors::error_from_response;
use crate::transport::{Transport, read_body};

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
        let (code, status) = status_code(err);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("long-running operation failed without a message")
            .to_string();
        return Err(GwsError::Api {
            code,
            message: format!(
                "Operation {} failed ({status}): {message}",
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

/// The HTTP status and gRPC status name for an operation's `error`.
///
/// `error` is a `google.rpc.Status`: its `code` is a gRPC code, not an HTTP
/// status. A missing or unknown code is reported as UNKNOWN (500); callers
/// still carry the operation's own message.
pub(crate) fn status_code(err: &Value) -> (u16, &'static str) {
    err.get("code")
        .and_then(Value::as_u64)
        .and_then(grpc_to_http)
        .unwrap_or((500, "UNKNOWN"))
}

/// The HTTP status and name Google maps a `google.rpc.Code` to
/// (`google/rpc/code.proto`); `None` for a code outside that enum.
fn grpc_to_http(code: u64) -> Option<(u16, &'static str)> {
    Some(match code {
        1 => (499, "CANCELLED"),
        2 => (500, "UNKNOWN"),
        3 => (400, "INVALID_ARGUMENT"),
        4 => (504, "DEADLINE_EXCEEDED"),
        5 => (404, "NOT_FOUND"),
        6 => (409, "ALREADY_EXISTS"),
        7 => (403, "PERMISSION_DENIED"),
        8 => (429, "RESOURCE_EXHAUSTED"),
        9 => (400, "FAILED_PRECONDITION"),
        10 => (409, "ABORTED"),
        11 => (400, "OUT_OF_RANGE"),
        12 => (501, "UNIMPLEMENTED"),
        13 => (500, "INTERNAL"),
        14 => (503, "UNAVAILABLE"),
        15 => (500, "DATA_LOSS"),
        16 => (401, "UNAUTHENTICATED"),
        // 0 is OK, which an `error` never carries.
        _ => return None,
    })
}

/// The error for `--wait` on an API without `operations.get`.
pub(crate) fn no_operations_get(doc: &RestDescription) -> GwsError {
    GwsError::Validation(format!(
        "--wait: the {} API has no operations.get method to poll",
        doc.name
    ))
}

/// How to poll an operation: the API's `operations.get`, the parameter that
/// takes the operation's name, and the method's other path parameters.
pub(crate) struct PollTarget<'a> {
    method: &'a RestMethod,
    op_param: String,
    params: Map<String, Value>,
}

/// The API's `operations.get`, the path parameter that takes the
/// operation's name, and the getter's other path parameters, which come
/// from the request's parameters of the same name.
struct PollShape<'a> {
    method: &'a RestMethod,
    op_param: String,
    others: Vec<&'a str>,
}

/// Whether `--wait` applies to `method`: the API's `operations.get` returns
/// the schema `method` returns, and every other path parameter that getter
/// needs is a parameter of `method`.
///
/// A response schema whose name merely ends in `Operation` is not enough.
/// Compute Engine operations are polled through `zoneOperations`,
/// `regionOperations` or `globalOperations` (there is no `operations.get`),
/// a Cloud DNS `Operation` has no `name` to poll, and an API Hub
/// `ApiOperation` describes an HTTP operation of a cataloged API.
pub(crate) fn can_wait(doc: &RestDescription, method: &RestMethod) -> bool {
    let schema = |m: &RestMethod| m.response.as_ref().and_then(|r| r.schema_ref.clone());
    let Some(returned) = schema(method) else {
        return false;
    };
    // An API whose operations cannot be polled is the answer here, not a
    // failure: the flag is not offered, and `--wait` is an unknown flag.
    let Ok(shape) = poll_shape(doc) else {
        return false;
    };
    schema(shape.method).as_deref() == Some(returned.as_str())
        && shape
            .others
            .iter()
            .all(|p| method.parameters.contains_key(*p))
}

/// Work out how to poll an operation started by a request with
/// `request_params`, or refuse. Checked before the request is sent, so
/// `--wait` fails before it starts an operation it could not poll.
pub(crate) fn poll_target<'a>(
    doc: &'a RestDescription,
    request_params: &Map<String, Value>,
) -> Result<PollTarget<'a>, GwsError> {
    let PollShape {
        method: get,
        op_param,
        others,
    } = poll_shape(doc)?;
    let id = get.id.as_deref().unwrap_or("operations.get");
    let template = path_template(get);
    let mut params = Map::new();
    for p in others {
        let value = request_params.get(p).ok_or_else(|| {
            GwsError::Validation(format!(
                "--wait: polling with {id} ({template}) needs path parameter '{p}', which \
                 this request does not have"
            ))
        })?;
        params.insert(p.to_string(), value.clone());
    }
    Ok(PollTarget {
        method: get,
        op_param,
        params,
    })
}

/// Work out how this API's operations are polled, or refuse.
///
/// A nested `operations.get` (storage `b/{bucket}/operations/{operationId}`)
/// takes the operation in the placeholder that follows `operations/`.
fn poll_shape(doc: &RestDescription) -> Result<PollShape<'_>, GwsError> {
    let get = find_operations_get(doc).ok_or_else(|| no_operations_get(doc))?;
    let id = get.id.as_deref().unwrap_or("operations.get");
    let template = path_template(get);
    let mut path_params: Vec<&str> = extract_template_path_parameters(template)
        .into_iter()
        .collect();
    path_params.sort_unstable();
    let op_param = match path_params.as_slice() {
        [] => get
            .parameter_order
            .first()
            .cloned()
            .unwrap_or_else(|| "name".to_string()),
        [only] => (*only).to_string(),
        many => {
            let after_operations: Vec<&str> = many
                .iter()
                .copied()
                .filter(|p| {
                    template.contains(&format!("operations/{{{p}}}"))
                        || template.contains(&format!("operations/{{+{p}}}"))
                })
                .collect();
            match after_operations.as_slice() {
                [p] => (*p).to_string(),
                _ => {
                    return Err(GwsError::Validation(format!(
                        "--wait: cannot tell which path parameter of {id} ({template}) takes \
                         the operation name"
                    )));
                }
            }
        }
    };
    let others = path_params.into_iter().filter(|p| *p != op_param).collect();
    Ok(PollShape {
        method: get,
        op_param,
        others,
    })
}

/// Poll `op` until done or the timeout elapses; returns the final operation.
///
/// The request that returned `op` was already accepted, so every failure
/// from here on names the operation: the caller can check on it instead of
/// rerunning the request and starting it a second time.
pub(crate) async fn wait(
    transport: &Transport,
    doc: &RestDescription,
    request_params: &Map<String, Value>,
    op: Value,
    cfg: &WaitConfig,
) -> Result<Value, GwsError> {
    if is_done(&op) {
        return Ok(op);
    }
    let name = op
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GwsError::other(anyhow::anyhow!("operation has no name to poll")))?
        .to_string();
    poll(transport, doc, request_params, op, &name, cfg)
        .await
        .map_err(|e| polling_failed(e, &name))
}

/// Prefix `err` with the operation that was started. The error keeps its
/// class, and so its exit code.
fn polling_failed(err: GwsError, name: &str) -> GwsError {
    let context = format!(
        "--wait: the request was accepted and started operation {name}, but waiting for it failed \
         (do not rerun the request; check the operation with the API's operations.get)"
    );
    match err {
        GwsError::Validation(m) => GwsError::Validation(format!("{context}: {m}")),
        GwsError::Auth(m) => GwsError::Auth(format!("{context}: {m}")),
        GwsError::Config(m) => GwsError::Config(format!("{context}: {m}")),
        GwsError::CredentialStore(m) => GwsError::CredentialStore(format!("{context}: {m}")),
        other => crate::transport::errors::with_context(other, &context),
    }
}

async fn poll(
    transport: &Transport,
    doc: &RestDescription,
    request_params: &Map<String, Value>,
    mut op: Value,
    name: &str,
    cfg: &WaitConfig,
) -> Result<Value, GwsError> {
    let PollTarget {
        method: get,
        op_param,
        mut params,
    } = poll_target(doc, request_params)?;
    let name = name.to_string();
    // With a plain `{name}` segment (drive `operations/{name}`), pass the id,
    // not the full `operations/<id>` resource name.
    let plain_segment = !path_template(get).contains(&format!("{{+{op_param}}}"));
    let value = if plain_segment {
        name.rsplit('/').next().unwrap_or(&name).to_string()
    } else {
        name.clone()
    };
    params.insert(op_param, Value::String(value));
    let url = build_url(doc, get, &params, UrlTarget::Method, transport.endpoints())?;

    let started = Instant::now();
    let mut interval = cfg.initial_interval;
    let auth = transport.auth_method()?;
    while !is_done(&op) {
        // Poll until the deadline itself: when the next interval would pass
        // it, sleep only what is left and poll once more at the deadline.
        let remaining = cfg.timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(GwsError::other(anyhow::anyhow!(
                "the operation did not finish within {}s; last state: {}",
                cfg.timeout.as_secs(),
                op.get("metadata").cloned().unwrap_or(json!(null))
            )));
        }
        tracing::info!("Waiting for operation {name}...");
        tokio::time::sleep(interval.min(remaining)).await;
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
    fn failed_operation_maps_grpc_codes_to_http_statuses() {
        let failed = |code: u64| {
            operation_result(json!({"name": "o", "done": true,
                                    "error": {"code": code, "message": "m"}}))
            .unwrap_err()
        };
        // google.rpc.Code 14 UNAVAILABLE is transient: HTTP 503, exit 6.
        let err = failed(14);
        assert_eq!(err.to_json()["error"]["code"], 503, "{err:?}");
        assert!(err.is_retryable());
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API_RETRYABLE);
        // 8 RESOURCE_EXHAUSTED is a quota error: HTTP 429, exit 6.
        let err = failed(8);
        assert_eq!(err.to_json()["error"]["code"], 429, "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API_RETRYABLE);
        // 9 FAILED_PRECONDITION is a permanent client error: HTTP 400, exit 1.
        let err = failed(9);
        assert_eq!(err.to_json()["error"]["code"], 400, "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
        assert!(err.to_string().contains("FAILED_PRECONDITION"), "{err}");
        // 5 NOT_FOUND and 7 PERMISSION_DENIED keep their HTTP meaning.
        assert_eq!(failed(5).to_json()["error"]["code"], 404);
        assert_eq!(failed(7).to_json()["error"]["code"], 403);
        // A code outside google.rpc.Code is an unknown server failure.
        assert_eq!(failed(99).to_json()["error"]["code"], 500);
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

    fn wait_doc(getter: Option<Value>, methods: Value) -> RestDescription {
        let mut resources = json!({"things": {"methods": methods}});
        if let Some(get) = getter {
            resources["projects"] = json!({"resources": {"operations": {"methods": {"get": get}}}});
        }
        serde_json::from_value(json!({"name": "x", "version": "v1", "rootUrl": "https://x.googleapis.com/", "servicePath": "", "resources": resources}))
            .unwrap()
    }

    fn path_param() -> Value {
        json!({"type": "string", "location": "path", "required": true})
    }

    #[test]
    fn wait_is_offered_only_where_the_operation_can_be_polled() {
        let getter = json!({
            "httpMethod": "GET", "path": "v1/projects/{project}/operations/{operation}",
            "parameters": {"project": path_param(), "operation": path_param()},
            "response": {"$ref": "Operation"}
        });
        let methods = json!({
            "create": {"httpMethod": "POST", "path": "v1/{project}/things", "parameters": {"project": path_param()}, "response": {"$ref": "Operation"}},
            "patch": {"httpMethod": "POST", "path": "v1/{project}/things", "parameters": {"project": path_param()}, "response": {"$ref": "ApiOperation"}},
            "orphan": {"httpMethod": "POST", "path": "v1/{project}/things", "parameters": {}, "response": {"$ref": "Operation"}},
            "delete": {"httpMethod": "POST", "path": "v1/{project}/things", "parameters": {"project": path_param()}}
        });
        let doc = wait_doc(Some(getter), methods.clone());
        let method = |name: &str| &doc.resources["things"].methods[name];
        // The getter returns the same schema, and every parameter it needs
        // besides the operation itself is one the method has.
        assert!(can_wait(&doc, method("create")));
        // A different schema whose name only ends in `Operation`.
        assert!(!can_wait(&doc, method("patch")));
        // The getter needs `project`, which the method cannot supply.
        assert!(!can_wait(&doc, method("orphan")));
        // No response at all.
        assert!(!can_wait(&doc, method("delete")));

        // An API without `operations.get` (Compute Engine polls through
        // `zoneOperations.get` and friends) offers `--wait` nowhere.
        let doc = wait_doc(None, methods);
        assert!(!can_wait(&doc, &doc.resources["things"].methods["create"]));
    }
}
