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

//! Model Armor screening of API responses (`--sanitize`).
//!
//! Fails closed: in `block` mode any sanitization failure suppresses output
//! and returns an error. In `warn` mode the response is still emitted but is
//! annotated with `"_sanitization": {"error": ...}` so the gap is visible.
//!
//! Merge point with helpers-gmail: this becomes a call to
//! `helpers::modelarmor::sanitize_value`.

use serde_json::{Value, json};

use super::errors::other;
use crate::error::GwsError;
use crate::helpers::modelarmor::{SanitizationResult, SanitizeMode};
use crate::output::sanitize_for_terminal;

/// Screen `value` with template `template` using `check`.
pub(crate) async fn screen(
    template: &str,
    mode: &SanitizeMode,
    mut value: Value,
    check: impl AsyncFnOnce(&str, &str) -> Result<SanitizationResult, GwsError>,
) -> Result<Value, GwsError> {
    let text = serde_json::to_string(&value).map_err(|e| {
        other(anyhow::anyhow!(
            "failed to serialize response for Model Armor: {e}"
        ))
    })?;
    let annotation = match check(template, &text).await {
        Ok(result) => {
            if result.filter_match_state == "MATCH_FOUND" {
                eprintln!(
                    "warning: Model Armor: prompt injection detected (filterMatchState: MATCH_FOUND)"
                );
                if *mode == SanitizeMode::Block {
                    return Err(other(anyhow::anyhow!(
                        "Content blocked by Model Armor (filterMatchState: MATCH_FOUND, invocationResult: {})",
                        result.invocation_result
                    )));
                }
            }
            serde_json::to_value(&result).map_err(|e| {
                other(anyhow::anyhow!(
                    "failed to serialize Model Armor result: {e}"
                ))
            })?
        }
        Err(e) => {
            if *mode == SanitizeMode::Block {
                return Err(other(anyhow::anyhow!(
                    "Model Armor sanitization failed and --sanitize mode is block, so the response was suppressed: {e}"
                )));
            }
            eprintln!(
                "warning: Model Armor sanitization failed; the response was NOT screened: {}",
                sanitize_for_terminal(&e.to_string())
            );
            json!({ "error": e.to_string() })
        }
    };
    match value.as_object_mut() {
        Some(obj) => {
            obj.insert("_sanitization".to_string(), annotation);
        }
        None => {
            value = json!({ "response": value, "_sanitization": annotation });
        }
    }
    Ok(value)
}

/// Screen with the real Model Armor API.
pub(crate) async fn screen_with_model_armor(
    template: &str,
    mode: &SanitizeMode,
    value: Value,
) -> Result<Value, GwsError> {
    screen(template, mode, value, async |t: &str, text: &str| {
        crate::helpers::modelarmor::sanitize_text(t, text).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(state: &str) -> SanitizationResult {
        SanitizationResult {
            filter_match_state: state.to_string(),
            filter_results: json!({}),
            invocation_result: "SUCCESS".to_string(),
        }
    }

    #[tokio::test]
    async fn block_mode_fails_closed_on_error() {
        let err = screen(
            "t",
            &SanitizeMode::Block,
            json!({"a": 1}),
            async |_: &str, _: &str| {
                Err(GwsError::Api {
                    code: 500,
                    message: "backend".into(),
                    reason: "x".into(),
                    enable_url: None,
                })
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("suppressed"));
    }

    #[tokio::test]
    async fn warn_mode_annotates_error() {
        let v = screen(
            "t",
            &SanitizeMode::Warn,
            json!({"a": 1}),
            async |_: &str, _: &str| Err(GwsError::Validation("boom".into())),
        )
        .await
        .unwrap();
        assert_eq!(v["a"], 1);
        assert!(
            v["_sanitization"]["error"]
                .as_str()
                .unwrap()
                .contains("boom")
        );
    }

    #[tokio::test]
    async fn match_found_blocks_or_annotates() {
        let err = screen(
            "t",
            &SanitizeMode::Block,
            json!({}),
            async |_: &str, _: &str| Ok(result("MATCH_FOUND")),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("blocked"));

        let v = screen(
            "t",
            &SanitizeMode::Warn,
            json!([1]),
            async |_: &str, _: &str| Ok(result("MATCH_FOUND")),
        )
        .await
        .unwrap();
        assert_eq!(v["response"], json!([1]));
        assert_eq!(v["_sanitization"]["filterMatchState"], "MATCH_FOUND");
    }
}
