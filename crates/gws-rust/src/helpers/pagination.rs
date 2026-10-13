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

//! The page-token rule shared by the helpers' hand-written pagination loops:
//! an absent or empty `nextPageToken` means the last page, and a token the
//! loop already followed is an error (it would request the same pages
//! forever).

use std::collections::HashSet;

use serde_json::Value;

use crate::error::GwsError;

/// The page tokens one pagination loop has followed.
#[derive(Debug, Default)]
pub(crate) struct PageTokens {
    seen: HashSet<String>,
}

impl PageTokens {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The token for the page after `page`: `None` on the last page (no or an
    /// empty `nextPageToken`), an error when the API repeats a token this
    /// loop already followed. `context` prefixes the error message.
    pub(crate) fn next(&mut self, page: &Value, context: &str) -> Result<Option<String>, GwsError> {
        match page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
        {
            None => Ok(None),
            Some(t) if !self.seen.insert(t.to_string()) => Err(GwsError::other(format!(
                "{context}: the API returned the nextPageToken '{t}' twice; aborting pagination"
            ))),
            Some(t) => Ok(Some(t.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_or_missing_token_is_the_last_page() {
        let mut tokens = PageTokens::new();
        assert_eq!(tokens.next(&json!({}), "ctx").unwrap(), None);
        assert_eq!(
            tokens.next(&json!({"nextPageToken": ""}), "ctx").unwrap(),
            None
        );
    }

    #[test]
    fn a_followed_token_is_an_error_even_after_others() {
        let mut tokens = PageTokens::new();
        let a = json!({"nextPageToken": "A"});
        let b = json!({"nextPageToken": "B"});
        assert_eq!(tokens.next(&a, "ctx").unwrap().as_deref(), Some("A"));
        assert_eq!(tokens.next(&b, "ctx").unwrap().as_deref(), Some("B"));
        let err = tokens.next(&a, "ctx").unwrap_err();
        assert!(err.to_string().contains("ctx:"), "{err}");
        assert!(err.to_string().contains("'A' twice"), "{err}");
    }
}
