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

//! Delivery of a finished RFC 5322 message: send or save as draft.

use super::api::GMAIL_UPLOAD_BASE;
use super::prelude::*;
use crate::confirm::{self, Impact};
use crate::formatter::{OutputFormat, format_value};

/// Whether the message is sent or saved as a draft, and whether this is a dry run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Delivery {
    pub draft: bool,
    pub dry_run: bool,
    pub format: OutputFormat,
}

impl Delivery {
    pub(super) fn from_matches(matches: &ArgMatches) -> Result<Self, GwsError> {
        Ok(Self {
            draft: crate::args::flag(matches, "draft")?,
            dry_run: crate::args::dry_run(matches)?,
            format: crate::helpers::http::output_format(matches)?,
        })
    }

    /// Apply the SEC-25 confirmation gate. Sending mail is an outbound action;
    /// saving a draft is not.
    pub(super) fn confirm(&self, matches: &ArgMatches, what: &str) -> Result<(), GwsError> {
        if self.draft {
            return Ok(());
        }
        confirm::confirm(matches, Impact::Outbound, what)
    }
}

/// Build the JSON metadata part for the upload endpoint.
///
/// * `users.messages.send`: `{"threadId": "..."}` when replying/forwarding, else `{}`.
/// * `users.drafts.create`: `{"message": {"threadId": "..."}}` or `{"message": {}}`.
pub(super) fn build_send_metadata(thread_id: Option<&str>, draft: bool) -> Value {
    let message = match thread_id {
        Some(id) => json!({ "threadId": id }),
        None => json!({}),
    };
    if draft {
        json!({ "message": message })
    } else {
        message
    }
}

/// Describe the upload request for `--dry-run` without authenticating.
pub(super) fn dry_run_plan(raw_message: &str, thread_id: Option<&str>, draft: bool) -> Value {
    let path = if draft { "drafts" } else { "messages/send" };
    json!({
        "dry_run": true,
        "method": "POST",
        "url": format!("{GMAIL_UPLOAD_BASE}/users/me/{path}"),
        "query_params": { "uploadType": "multipart" },
        "metadata": build_send_metadata(thread_id, draft),
        "raw_message": raw_message,
    })
}

/// Send the message (or save the draft) and print the API response.
///
/// In dry-run mode the request is printed instead and `api` may be `None`.
pub(super) async fn deliver(
    api: Option<&GmailApi>,
    delivery: Delivery,
    raw_message: &str,
    thread_id: Option<&str>,
) -> Result<(), GwsError> {
    if delivery.dry_run {
        let plan = dry_run_plan(raw_message, thread_id, delivery.draft);
        return crate::output::emit(&format_value(&plan, &delivery.format)?);
    }
    let api =
        api.ok_or_else(|| GwsError::other("internal error: no Gmail client for a real send"))?;
    let metadata = build_send_metadata(thread_id, delivery.draft);
    let response = api
        .upload_raw(raw_message.as_bytes(), &metadata, delivery.draft)
        .await?;
    crate::output::emit(&format_value(&response, &delivery.format)?)?;

    if delivery.draft {
        let id = response
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| GwsError::other("drafts.create response has no draft \"id\""))?;
        tracing::info!(
            "Draft saved. Send it with: gwsr gmail users drafts send --params '{{\"userId\":\"me\"}}' --json '{{\"id\":\"{}\"}}'",
            sanitize_for_terminal(id)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::mock_api;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_build_send_metadata() {
        assert_eq!(
            build_send_metadata(Some("t1"), false),
            json!({"threadId": "t1"})
        );
        assert_eq!(build_send_metadata(None, false), json!({}));
        assert_eq!(
            build_send_metadata(Some("t1"), true),
            json!({"message": {"threadId": "t1"}})
        );
        assert_eq!(build_send_metadata(None, true), json!({"message": {}}));
    }

    #[test]
    fn test_dry_run_plan_targets_upload_endpoint() {
        let plan = dry_run_plan("raw", Some("t"), false);
        assert_eq!(
            plan["url"],
            "https://gmail.googleapis.com/upload/gmail/v1/users/me/messages/send"
        );
        assert_eq!(plan["metadata"]["threadId"], "t");
        let plan = dry_run_plan("raw", None, true);
        assert!(plan["url"].as_str().unwrap().ends_with("/users/me/drafts"));
    }

    #[tokio::test]
    async fn test_deliver_sends_once_and_never_retries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/gmail/v1/users/me/messages/send"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        let api = mock_api(&server);
        let delivery = Delivery {
            draft: false,
            dry_run: false,
            format: OutputFormat::Json,
        };
        assert!(deliver(Some(&api), delivery, "raw", None).await.is_err());
    }

    #[tokio::test]
    async fn test_deliver_draft_requires_id_in_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/gmail/v1/users/me/drafts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let api = mock_api(&server);
        let delivery = Delivery {
            draft: true,
            dry_run: false,
            format: OutputFormat::Json,
        };
        let err = deliver(Some(&api), delivery, "raw", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("draft"));
    }
}
