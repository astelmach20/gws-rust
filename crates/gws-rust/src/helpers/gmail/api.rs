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

//! Typed wrapper around the Gmail REST endpoints used by the helpers.
//!
//! Every call reports failures with the real HTTP status. Base URLs are
//! injectable so the helpers can be exercised against a mock server.

use super::prelude::*;
use crate::transport::Transport;
use gws_rust_core::client::Idempotency;
use reqwest::Method;

pub(super) const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";
pub(super) const GMAIL_UPLOAD_BASE: &str = "https://gmail.googleapis.com/upload/gmail/v1";

/// Gmail API client bound to the authenticated user (`users/me`).
#[derive(Clone)]
pub(super) struct GmailApi {
    rest: Transport,
    base: String,
    upload_base: String,
}

/// A Gmail label (user or system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Label {
    pub id: String,
    pub name: String,
}

impl GmailApi {
    pub(super) fn new(rest: Transport) -> Self {
        Self::with_bases(rest, GMAIL_API_BASE, GMAIL_UPLOAD_BASE)
    }

    pub(super) fn with_bases(rest: Transport, base: &str, upload_base: &str) -> Self {
        Self {
            rest,
            base: base.trim_end_matches('/').to_string(),
            upload_base: upload_base.trim_end_matches('/').to_string(),
        }
    }

    pub(super) fn url(&self, path: &str) -> String {
        format!("{}/users/me/{path}", self.base)
    }

    pub(super) fn message_url(&self, id: &str) -> String {
        self.url(&format!(
            "messages/{}",
            crate::validate::encode_path_segment(id)
        ))
    }

    fn thread_url(&self, id: &str) -> String {
        self.url(&format!(
            "threads/{}",
            crate::validate::encode_path_segment(id)
        ))
    }

    /// `users.messages.get` with the given format and optional metadata headers.
    pub(super) async fn get_message(
        &self,
        id: &str,
        format: &str,
        metadata_headers: &[&str],
    ) -> Result<Value, GwsError> {
        let mut query: Vec<(&str, String)> = vec![("format", format.to_string())];
        query.extend(
            metadata_headers
                .iter()
                .map(|h| ("metadataHeaders", (*h).to_string())),
        );
        self.rest
            .get_json(
                &self.message_url(id),
                &query,
                &format!("Failed to fetch message {id}"),
            )
            .await
    }

    /// Fetch a message in `full` format and parse it for reply/forward/read.
    pub(super) async fn get_original_message(&self, id: &str) -> Result<OriginalMessage, GwsError> {
        let msg = self.get_message(id, "full", &[]).await?;
        parse_original_message(&msg)
    }

    /// One page of `users.messages.list`.
    pub(super) async fn list_messages(
        &self,
        query: Option<&str>,
        page_token: Option<&str>,
        max_results: u32,
        include_spam_trash: bool,
    ) -> Result<Value, GwsError> {
        let mut params: Vec<(&str, String)> = vec![("maxResults", max_results.to_string())];
        if let Some(q) = query {
            params.push(("q", q.to_string()));
        }
        if let Some(t) = page_token {
            params.push(("pageToken", t.to_string()));
        }
        if include_spam_trash {
            params.push(("includeSpamTrash", "true".to_string()));
        }
        self.rest
            .get_json(&self.url("messages"), &params, "Failed to list messages")
            .await
    }

    /// Download and decode one attachment's bytes.
    pub(super) async fn get_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, GwsError> {
        let url = format!(
            "{}/attachments/{}",
            self.message_url(message_id),
            crate::validate::encode_path_segment(attachment_id)
        );
        let body = self
            .rest
            .get_json(
                &url,
                &[],
                &format!("Failed to fetch attachment {attachment_id} from message {message_id}"),
            )
            .await?;
        let data = body.get("data").and_then(Value::as_str).ok_or_else(|| {
            GwsError::other(format!(
                "Attachment response missing 'data' field for {attachment_id}"
            ))
        })?;
        decode_base64url(data).map_err(|e| {
            GwsError::other(format!("Failed to decode attachment {attachment_id}: {e}"))
        })
    }

    pub(super) async fn send_as_list(&self) -> Result<Value, GwsError> {
        self.rest
            .get_json(
                &self.url("settings/sendAs"),
                &[],
                "Failed to fetch sendAs settings",
            )
            .await
    }

    pub(super) async fn profile(&self) -> Result<Value, GwsError> {
        self.rest
            .get_json(&self.url("profile"), &[], "Failed to fetch Gmail profile")
            .await
    }

    /// Add/remove labels on messages. Uses `batchModify` for more than one ID.
    pub(super) async fn modify_messages(
        &self,
        ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<Value, GwsError> {
        let (url, body) = self.modify_messages_request(ids, add, remove);
        self.rest
            .json(
                Method::POST,
                &url,
                &[],
                Some(&body),
                Idempotency::Idempotent,
                "Failed to modify message labels",
            )
            .await
    }

    pub(super) fn modify_messages_request(
        &self,
        ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> (String, Value) {
        if let [single] = ids {
            (
                format!("{}/modify", self.message_url(single)),
                json!({ "addLabelIds": add, "removeLabelIds": remove }),
            )
        } else {
            (
                self.url("messages/batchModify"),
                json!({ "ids": ids, "addLabelIds": add, "removeLabelIds": remove }),
            )
        }
    }

    pub(super) fn modify_thread_url(&self, id: &str) -> String {
        format!("{}/modify", self.thread_url(id))
    }

    pub(super) async fn modify_thread(
        &self,
        id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<Value, GwsError> {
        let body = json!({ "addLabelIds": add, "removeLabelIds": remove });
        self.rest
            .json(
                Method::POST,
                &self.modify_thread_url(id),
                &[],
                Some(&body),
                Idempotency::Idempotent,
                &format!("Failed to modify thread {id}"),
            )
            .await
    }

    pub(super) fn trash_url(&self, kind: TargetKind, id: &str) -> String {
        match kind {
            TargetKind::Message => format!("{}/trash", self.message_url(id)),
            TargetKind::Thread => format!("{}/trash", self.thread_url(id)),
        }
    }

    pub(super) async fn trash(&self, kind: TargetKind, id: &str) -> Result<Value, GwsError> {
        self.rest
            .json(
                Method::POST,
                &self.trash_url(kind, id),
                &[],
                None,
                Idempotency::Idempotent,
                &format!("Failed to trash {} {id}", kind.noun()),
            )
            .await
    }

    pub(super) async fn list_labels(&self) -> Result<Vec<Label>, GwsError> {
        let body = self
            .rest
            .get_json(&self.url("labels"), &[], "Failed to list labels")
            .await?;
        parse_labels(&body)
    }

    pub(super) async fn list_filters(&self) -> Result<Value, GwsError> {
        self.rest
            .get_json(&self.url("settings/filters"), &[], "Failed to list filters")
            .await
    }

    pub(super) async fn create_filter(&self, filter: &Value) -> Result<Value, GwsError> {
        self.rest
            .json(
                Method::POST,
                &self.url("settings/filters"),
                &[],
                Some(filter),
                Idempotency::NonIdempotent,
                "Failed to create filter",
            )
            .await
    }

    pub(super) fn filter_url(&self, id: &str) -> String {
        self.url(&format!(
            "settings/filters/{}",
            crate::validate::encode_path_segment(id)
        ))
    }

    pub(super) async fn delete_filter(&self, id: &str) -> Result<Value, GwsError> {
        self.rest
            .json(
                Method::DELETE,
                &self.filter_url(id),
                &[],
                None,
                Idempotency::Idempotent,
                &format!("Failed to delete filter {id}"),
            )
            .await
    }

    /// One page of `users.history.list` for `messageAdded` events.
    pub(super) async fn history(
        &self,
        start_history_id: u64,
        page_token: Option<&str>,
    ) -> Result<Value, GwsError> {
        let mut query: Vec<(&str, String)> = vec![
            ("startHistoryId", start_history_id.to_string()),
            ("historyTypes", "messageAdded".to_string()),
        ];
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_string()));
        }
        self.rest
            .get_json(&self.url("history"), &query, "Failed to list history")
            .await
    }

    pub(super) async fn get_thread_minimal(&self, id: &str) -> Result<Value, GwsError> {
        self.rest
            .get_json(
                &self.thread_url(id),
                &[("format", "minimal".to_string())],
                &format!("Failed to fetch thread {id}"),
            )
            .await
    }

    pub(super) async fn watch(&self, body: &Value) -> Result<Value, GwsError> {
        self.rest
            .json(
                Method::POST,
                &self.url("watch"),
                &[],
                Some(body),
                Idempotency::Idempotent,
                "gmail.users.watch failed",
            )
            .await
    }

    /// The upload URL for sending a message or creating a draft.
    pub(super) fn upload_url(&self, draft: bool) -> String {
        let path = if draft { "drafts" } else { "messages/send" };
        format!("{}/users/me/{path}", self.upload_base)
    }

    /// Send a raw RFC 5322 message (or save it as a draft) with a
    /// `multipart/related` upload.
    ///
    /// Sent **exactly once**: a timeout or 5xx is reported, never retried,
    /// because the message may already have been delivered.
    pub(super) async fn upload_raw(
        &self,
        raw: &[u8],
        metadata: &Value,
        draft: bool,
    ) -> Result<Value, GwsError> {
        let boundary = format!("gwsr_{:032x}", rand::random::<u128>());
        let body = build_related_upload(&boundary, metadata, raw)?;
        let context = if draft {
            "Failed to create draft"
        } else {
            "Failed to send message"
        };
        let resp = self
            .rest
            .send_ok(
                Method::POST,
                &self.upload_url(draft),
                Idempotency::NonIdempotent,
                context,
                None,
                |rb| {
                    rb.query(&[("uploadType", "multipart")])
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            format!("multipart/related; boundary={boundary}"),
                        )
                        .body(body.clone())
                },
            )
            .await?;
        crate::transport::json_body(resp, self.rest.retry.response_timeout, context).await
    }
}

/// Whether a helper targets messages or whole threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TargetKind {
    Message,
    Thread,
}

impl TargetKind {
    pub(super) fn noun(self) -> &'static str {
        match self {
            TargetKind::Message => "message",
            TargetKind::Thread => "thread",
        }
    }
}

/// Build a `multipart/related` body: JSON metadata part + `message/rfc822` part.
pub(super) fn build_related_upload(
    boundary: &str,
    metadata: &Value,
    raw: &[u8],
) -> Result<Vec<u8>, GwsError> {
    let meta = serde_json::to_string(metadata)
        .map_err(|e| GwsError::other(format!("Failed to serialize upload metadata: {e}")))?;
    let mut body = Vec::with_capacity(raw.len() + meta.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n\
             --{boundary}\r\nContent-Type: message/rfc822\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(raw);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(body)
}

/// Parse a `users.labels.list` response.
pub(super) fn parse_labels(body: &Value) -> Result<Vec<Label>, GwsError> {
    let Some(labels) = body.get("labels") else {
        return Ok(Vec::new());
    };
    let labels = labels
        .as_array()
        .ok_or_else(|| GwsError::other("labels.list response: 'labels' is not an array"))?;
    labels
        .iter()
        .map(|l| {
            let id = l.get("id").and_then(Value::as_str);
            let name = l.get("name").and_then(Value::as_str);
            match (id, name) {
                (Some(id), Some(name)) => Ok(Label {
                    id: id.to_string(),
                    name: name.to_string(),
                }),
                _ => Err(GwsError::other(format!(
                    "labels.list response contains a label without id/name: {l}"
                ))),
            }
        })
        .collect()
}

/// Resolve user-supplied label names or IDs to label IDs.
///
/// Matches an exact ID first, then a case-insensitive name. Unknown labels
/// are an error listing every unresolved value.
pub(super) fn resolve_label_ids(
    wanted: &[String],
    labels: &[Label],
) -> Result<Vec<String>, GwsError> {
    let mut ids = Vec::with_capacity(wanted.len());
    let mut missing = Vec::new();
    for w in wanted {
        if let Some(l) = labels.iter().find(|l| l.id == *w) {
            ids.push(l.id.clone());
        } else if let Some(l) = labels.iter().find(|l| l.name.eq_ignore_ascii_case(w)) {
            ids.push(l.id.clone());
        } else {
            missing.push(w.clone());
        }
    }
    if !missing.is_empty() {
        return Err(GwsError::Validation(format!(
            "Unknown Gmail label(s): {}. Create them first (gwsr gmail users labels create) \
             or check the name.",
            missing.join(", ")
        )));
    }
    Ok(ids)
}

/// Obtain an authenticated Gmail client for `scopes`.
pub(super) async fn authenticated(scopes: &[&str]) -> Result<GmailApi, GwsError> {
    Ok(GmailApi::new(Transport::for_scopes(scopes).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::mock_api;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn related_upload_has_both_parts() {
        let body =
            build_related_upload("B", &json!({"threadId": "t1"}), b"Subject: hi\r\n\r\nx").unwrap();
        let s = String::from_utf8(body).unwrap();
        assert!(s.starts_with("--B\r\nContent-Type: application/json"));
        assert!(s.contains(r#"{"threadId":"t1"}"#));
        assert!(s.contains("Content-Type: message/rfc822\r\n\r\nSubject: hi"));
        assert!(s.ends_with("\r\n--B--\r\n"));
    }

    #[test]
    fn resolve_label_ids_by_id_and_name() {
        let labels = vec![
            Label {
                id: "INBOX".into(),
                name: "INBOX".into(),
            },
            Label {
                id: "Label_1".into(),
                name: "Receipts".into(),
            },
        ];
        let ids = resolve_label_ids(&["receipts".into(), "INBOX".into()], &labels).unwrap();
        assert_eq!(ids, vec!["Label_1", "INBOX"]);
        let err = resolve_label_ids(&["Nope".into(), "Other".into()], &labels).unwrap_err();
        assert!(err.to_string().contains("Nope, Other"));
    }

    #[test]
    fn parse_labels_rejects_malformed_entries() {
        assert!(parse_labels(&json!({})).unwrap().is_empty());
        assert!(parse_labels(&json!({"labels": [{"id": "x"}]})).is_err());
        assert!(parse_labels(&json!({"labels": "x"})).is_err());
    }

    #[test]
    fn modify_request_uses_batch_for_many() {
        let h = "http://127.0.0.1:9";
        let api = GmailApi::with_bases(Transport::for_test(h), h, "http://127.0.0.1:9/u");
        let (url, body) = api.modify_messages_request(&["a".into()], &["L".into()], &[]);
        assert_eq!(url, "http://127.0.0.1:9/users/me/messages/a/modify");
        assert_eq!(body["addLabelIds"][0], "L");
        let (url, body) =
            api.modify_messages_request(&["a".into(), "b".into()], &[], &["INBOX".into()]);
        assert_eq!(url, "http://127.0.0.1:9/users/me/messages/batchModify");
        assert_eq!(body["ids"][1], "b");
    }

    #[tokio::test]
    async fn get_attachment_decodes_unpadded_base64url() {
        let server = MockServer::start().await;
        // "hello?" base64url without padding.
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m1/attachments/a1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": "aGVsbG8_"})))
            .mount(&server)
            .await;
        let api = mock_api(&server);
        assert_eq!(api.get_attachment("m1", "a1").await.unwrap(), b"hello?");
    }

    #[tokio::test]
    async fn upload_raw_posts_multipart_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/gmail/v1/users/me/messages/send"))
            .and(query_param("uploadType", "multipart"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let api = mock_api(&server);
        let err = api.upload_raw(b"raw", &json!({}), false).await.unwrap_err();
        match err {
            GwsError::Api { code, .. } => assert_eq!(code, 503),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_filter_posts_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/settings/filters"))
            .and(body_json(json!({"criteria": {"from": "a@b.c"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "f1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = mock_api(&server);
        let v = api
            .create_filter(&json!({"criteria": {"from": "a@b.c"}}))
            .await
            .unwrap();
        assert_eq!(v["id"], "f1");
    }
}
