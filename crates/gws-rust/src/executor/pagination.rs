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

//! Pagination: where the page token goes and how pages are split into items.

use serde_json::{Value, json};

use crate::discovery::{PageTokenLocation, RestDescription, RestMethod};
use crate::error::GwsError;

/// Put `token` where the method expects it.
pub(crate) fn place_token(
    location: PageTokenLocation,
    token: &str,
    query: &mut Vec<(String, String)>,
    body: &mut Option<Value>,
) -> Result<(), GwsError> {
    match location {
        PageTokenLocation::Query => {
            query.retain(|(k, _)| k != "pageToken");
            query.push(("pageToken".to_string(), token.to_string()));
        }
        PageTokenLocation::Body => {
            let obj = body.get_or_insert_with(|| json!({}));
            let map = obj.as_object_mut().ok_or_else(|| {
                GwsError::Validation(
                    "--page-all needs a JSON object request body to carry pageToken".to_string(),
                )
            })?;
            map.insert("pageToken".to_string(), Value::String(token.to_string()));
        }
        unknown => {
            return Err(GwsError::Discovery(format!(
                "unsupported pageToken location {unknown:?} in the Discovery Document"
            )));
        }
    }
    Ok(())
}

/// The `nextPageToken` of a page, if any (empty strings mean "no more pages").
pub(crate) fn next_token(page: &Value) -> Option<&str> {
    page.get("nextPageToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
}

/// Which array field of a page holds the items for `--page-items`.
///
/// `requested` wins. Otherwise the response schema's single array property is
/// used, falling back to the page's single array field.
pub(crate) fn item_field(
    doc: &RestDescription,
    method: &RestMethod,
    page: &Value,
    requested: Option<&str>,
) -> Result<String, GwsError> {
    let response_schema = method
        .response
        .as_ref()
        .and_then(|r| r.schema_ref.as_deref())
        .and_then(|name| doc.schemas.get(name));
    if let Some(field) = requested {
        // APIs omit empty list fields from a page, so a field missing from
        // the page is only an error when the response schema lacks it too;
        // otherwise a typo would silently print nothing.
        let in_page = page.get(field).is_some();
        if let Some(schema) = response_schema.filter(|s| !s.properties.is_empty())
            && !in_page
            && !schema.properties.contains_key(field)
        {
            let mut arrays: Vec<&str> = schema
                .properties
                .iter()
                .filter(|(_, p)| p.prop_type.as_deref() == Some("array"))
                .map(|(k, _)| k.as_str())
                .collect();
            arrays.sort_unstable();
            return Err(GwsError::Validation(format!(
                "--page-items: the response has no field '{field}'; its list fields are: {}",
                if arrays.is_empty() {
                    "(none)".to_string()
                } else {
                    arrays.join(", ")
                }
            )));
        }
        return Ok(field.to_string());
    }
    let schema_arrays: Vec<String> = response_schema
        .map(|s| {
            let mut v: Vec<String> = s
                .properties
                .iter()
                .filter(|(_, p)| p.prop_type.as_deref() == Some("array"))
                .map(|(k, _)| k.clone())
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default();
    if let [only] = schema_arrays.as_slice() {
        return Ok(only.clone());
    }
    let page_arrays: Vec<String> = page
        .as_object()
        .map(|o| {
            let mut v: Vec<String> = o
                .iter()
                .filter(|(_, v)| v.is_array())
                .map(|(k, _)| k.clone())
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default();
    let candidates = if schema_arrays.is_empty() {
        page_arrays
    } else {
        schema_arrays
    };
    match candidates.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(GwsError::Validation(
            "--page-items: the response has no array field to split into items".to_string(),
        )),
        many => Err(GwsError::Validation(format!(
            "--page-items: the response has several array fields ({}); choose one with --page-items=<FIELD>",
            many.join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{JsonSchema, JsonSchemaProperty, SchemaRef};
    use std::collections::HashMap;

    fn schema(props: &[(&str, &str)]) -> JsonSchema {
        JsonSchema {
            properties: props
                .iter()
                .map(|(k, t)| {
                    (
                        (*k).to_string(),
                        JsonSchemaProperty {
                            prop_type: Some((*t).to_string()),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        }
    }

    fn sref(name: &str) -> Option<SchemaRef> {
        Some(SchemaRef {
            schema_ref: Some(name.to_string()),
            parameter_name: None,
        })
    }

    #[test]
    fn places_tokens() {
        let mut q = vec![("pageToken".to_string(), "old".to_string())];
        let mut body = None;
        place_token(PageTokenLocation::Query, "t2", &mut q, &mut body).unwrap();
        assert_eq!(q, vec![("pageToken".to_string(), "t2".to_string())]);
        place_token(PageTokenLocation::Body, "t3", &mut q, &mut body).unwrap();
        assert_eq!(body, Some(json!({"pageToken": "t3"})));
        let mut bad = Some(json!([1]));
        assert!(place_token(PageTokenLocation::Body, "t", &mut q, &mut bad).is_err());
    }

    #[test]
    fn next_token_ignores_empty() {
        assert_eq!(next_token(&json!({"nextPageToken": "a"})), Some("a"));
        assert_eq!(next_token(&json!({"nextPageToken": ""})), None);
        assert_eq!(next_token(&json!({})), None);
    }

    #[test]
    fn item_field_resolution() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "FileList".to_string(),
            schema(&[("files", "array"), ("nextPageToken", "string")]),
        );
        schemas.insert(
            "Multi".to_string(),
            schema(&[("a", "array"), ("b", "array")]),
        );
        let doc = RestDescription {
            schemas,
            ..Default::default()
        };
        let files = RestMethod {
            response: sref("FileList"),
            ..Default::default()
        };
        let multi = RestMethod {
            response: sref("Multi"),
            ..Default::default()
        };
        let page = json!({"files": [], "other": []});
        assert_eq!(item_field(&doc, &files, &page, None).unwrap(), "files");
        assert_eq!(item_field(&doc, &multi, &page, Some("a")).unwrap(), "a");
        let err = item_field(&doc, &multi, &page, None).unwrap_err();
        assert!(err.to_string().contains("several array fields"));
        // No schema: fall back to the page's single array field.
        let bare = RestMethod::default();
        assert_eq!(
            item_field(&doc, &bare, &json!({"items": [1]}), None).unwrap(),
            "items"
        );
    }

    #[test]
    fn requested_item_field_is_checked_against_the_schema() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "FileList".to_string(),
            schema(&[("files", "array"), ("nextPageToken", "string")]),
        );
        let doc = RestDescription {
            schemas,
            ..Default::default()
        };
        let files = RestMethod {
            response: sref("FileList"),
            ..Default::default()
        };
        // An empty page may omit the list field: the schema still knows it.
        assert_eq!(
            item_field(&doc, &files, &json!({}), Some("files")).unwrap(),
            "files"
        );
        // A typo is rejected, naming the list fields.
        let err = item_field(&doc, &files, &json!({}), Some("file")).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(err.to_string().contains("files"), "{err}");
        // A field the page has but the schema lacks is trusted.
        assert_eq!(
            item_field(&doc, &files, &json!({"extra": []}), Some("extra")).unwrap(),
            "extra"
        );
        // Without a response schema there is nothing to check against.
        let bare = RestMethod::default();
        assert_eq!(
            item_field(&doc, &bare, &json!({}), Some("anything")).unwrap(),
            "anything"
        );
    }
}
