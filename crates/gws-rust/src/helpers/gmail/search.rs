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

//! `gmail +search`: paginated search returning full message metadata.
//! Also provides the shared search used by `+triage`.

use super::cli::{parse_optional_trimmed, value_or_default};
use super::prelude::*;
use crate::helpers::modelarmor::{SanitizeConfig, require_pass, sanitize_value};
use futures_util::stream::{self, StreamExt, TryStreamExt};

/// Gmail caps `maxResults` per list page at 500.
const MAX_PAGE_SIZE: u32 = 500;
/// Concurrent `messages.get` requests.
const FETCH_CONCURRENCY: usize = 10;
/// Headers requested in metadata format.
pub(super) const SUMMARY_HEADERS: &[&str] = &["From", "To", "Cc", "Subject", "Date"];

/// What to search for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchParams {
    pub query: Option<String>,
    pub max: u32,
    pub page_token: Option<String>,
    pub include_spam_trash: bool,
}

/// Search results in API order.
#[derive(Debug, Default)]
pub(super) struct SearchResults {
    /// `messages.get` responses (metadata format), in list order.
    pub messages: Vec<Value>,
    /// Present when more results exist beyond `max`.
    pub next_page_token: Option<String>,
    pub result_size_estimate: Option<u64>,
}

/// List message IDs page by page until `max` is reached or results run out,
/// then fetch metadata for each. Any failed request fails the whole search.
pub(super) async fn search(
    api: &GmailApi,
    params: &SearchParams,
) -> Result<SearchResults, GwsError> {
    let mut ids: Vec<String> = Vec::new();
    let mut page_token = params.page_token.clone();
    let mut result_size_estimate = None;
    let mut next_page_token = None;

    loop {
        // Saturating: more than u32::MAX collected IDs means none remain.
        let remaining = params
            .max
            .saturating_sub(u32::try_from(ids.len()).unwrap_or(u32::MAX));
        if remaining == 0 {
            break;
        }
        let page = api
            .list_messages(
                params.query.as_deref(),
                page_token.as_deref(),
                remaining.min(MAX_PAGE_SIZE),
                params.include_spam_trash,
            )
            .await?;
        if result_size_estimate.is_none() {
            result_size_estimate = page.get("resultSizeEstimate").and_then(Value::as_u64);
        }
        if let Some(messages) = page.get("messages") {
            let messages = messages.as_array().ok_or_else(|| {
                GwsError::other("messages.list response: 'messages' is not an array")
            })?;
            for m in messages {
                let id = m.get("id").and_then(Value::as_str).ok_or_else(|| {
                    GwsError::other(format!("messages.list entry without id: {m}"))
                })?;
                ids.push(id.to_string());
            }
        }
        let token = page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        match token {
            // Saturating, as above.
            Some(t) if u32::try_from(ids.len()).unwrap_or(u32::MAX) >= params.max => {
                next_page_token = Some(t);
                break;
            }
            Some(t) => page_token = Some(t),
            None => break,
        }
    }

    let messages = stream::iter(ids)
        .map(|id| async move { api.get_message(&id, "metadata", SUMMARY_HEADERS).await })
        .buffered(FETCH_CONCURRENCY)
        .try_collect()
        .await?;

    Ok(SearchResults {
        messages,
        next_page_token,
        result_size_estimate,
    })
}

/// Find a header value (case-insensitive) in a message resource's payload.
pub(super) fn header<'a>(msg: &'a Value, name: &str) -> &'a str {
    msg.get("payload")
        .and_then(|p| get_part_header(p, name))
        .unwrap_or("")
}

/// Convert `internalDate` (epoch millis as a string) to RFC 3339. The field
/// is informational in the summary: when Gmail omits it or it is not a
/// timestamp, the output shows `null` rather than failing the whole search.
fn internal_date_rfc3339(msg: &Value) -> Option<String> {
    let millis: i64 = msg.get("internalDate")?.as_str()?.parse().ok()?;
    chrono::DateTime::from_timestamp_millis(millis).map(|d| d.to_rfc3339())
}

/// Flatten a metadata-format message into the `+search` output shape.
pub(super) fn summarize(msg: &Value) -> Value {
    json!({
        "id": msg.get("id").cloned().unwrap_or(Value::Null),
        "threadId": msg.get("threadId").cloned().unwrap_or(Value::Null),
        "labelIds": msg.get("labelIds").cloned().unwrap_or_else(|| json!([])),
        "snippet": msg.get("snippet").cloned().unwrap_or(Value::Null),
        "sizeEstimate": msg.get("sizeEstimate").cloned().unwrap_or(Value::Null),
        "internalDate": internal_date_rfc3339(msg),
        "from": header(msg, "From"),
        "to": header(msg, "To"),
        "cc": header(msg, "Cc"),
        "subject": header(msg, "Subject"),
        "date": header(msg, "Date"),
    })
}

fn parse_search_args(matches: &ArgMatches) -> Result<SearchParams, GwsError> {
    Ok(SearchParams {
        query: parse_optional_trimmed(matches, "query")?,
        max: value_or_default::<u32>(matches, "max")?,
        page_token: parse_optional_trimmed(matches, "page-token")?,
        include_spam_trash: crate::args::flag(matches, "include-spam-trash")?,
    })
}

/// Dry-run description of the first list request.
pub(super) fn dry_run_list(matches: &ArgMatches, params: &SearchParams) -> Result<(), GwsError> {
    let mut query: Vec<(&str, String)> =
        vec![("maxResults", params.max.min(MAX_PAGE_SIZE).to_string())];
    if let Some(q) = &params.query {
        query.push(("q", q.clone()));
    }
    if let Some(t) = &params.page_token {
        query.push(("pageToken", t.clone()));
    }
    if params.include_spam_trash {
        query.push(("includeSpamTrash", "true".to_string()));
    }
    let url = format!("{}/users/me/messages", super::api::GMAIL_API_BASE);
    crate::helpers::http::print_dry_run(
        matches,
        vec![crate::helpers::http::dry_run_request(
            "GET", &url, &query, None,
        )],
    )
}

/// Build the `+search` output document.
fn search_output(params: &SearchParams, results: &SearchResults) -> Value {
    let mut out = json!({
        "messages": results.messages.iter().map(summarize).collect::<Vec<_>>(),
        "resultSizeEstimate": results.result_size_estimate,
        "query": params.query,
    });
    if let Some(t) = &results.next_page_token {
        out["nextPageToken"] = json!(t);
    }
    out
}

/// Handle the `+search` subcommand.
pub(super) async fn handle_search(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    let params = parse_search_args(matches)?;
    let format = crate::helpers::http::output_format(matches)?;
    if crate::args::dry_run(matches)? {
        return dry_run_list(matches, &params);
    }
    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let results = search(&api, &params).await?;
    if let Some(t) = &results.next_page_token {
        tracing::info!(
            "More results are available; continue with --page-token {}",
            sanitize_for_terminal(t)
        );
    }
    let output =
        require_pass(sanitize_value(sanitize_config, search_output(&params, &results)).await?)?;
    crate::output::emit(&crate::formatter::format_value(&output, &format)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::{helper_matches, mock_api};
    use wiremock::matchers::{method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn metadata(id: &str) -> Value {
        json!({
            "id": id,
            "threadId": format!("t-{id}"),
            "labelIds": ["INBOX"],
            "snippet": "hi",
            "internalDate": "1767225600000",
            "payload": { "headers": [
                { "name": "From", "value": "Alice <a@example.com>" },
                { "name": "subject", "value": format!("Subject {id}") }
            ]}
        })
    }

    async fn mount_messages(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path_regex(r"^/gmail/v1/users/me/messages/m\d+$"))
            .respond_with(|req: &wiremock::Request| {
                let id = req
                    .url
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                ResponseTemplate::new(200).set_body_json(metadata(&id))
            })
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn search_paginates_and_preserves_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{"id": "m3"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("maxResults", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{"id": "m1"}, {"id": "m2"}],
                "nextPageToken": "p2",
                "resultSizeEstimate": 3
            })))
            .mount(&server)
            .await;
        mount_messages(&server).await;

        let params = SearchParams {
            query: Some("x".into()),
            max: 5,
            page_token: None,
            include_spam_trash: false,
        };
        let results = search(&mock_api(&server), &params).await.unwrap();
        let ids: Vec<_> = results
            .messages
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        assert!(results.next_page_token.is_none());
        assert_eq!(results.result_size_estimate, Some(3));
    }

    #[tokio::test]
    async fn search_stops_at_max_and_reports_next_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .and(query_param("maxResults", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{"id": "m1"}, {"id": "m2"}],
                "nextPageToken": "more"
            })))
            .expect(1)
            .mount(&server)
            .await;
        mount_messages(&server).await;
        let params = SearchParams {
            query: None,
            max: 2,
            page_token: None,
            include_spam_trash: false,
        };
        let results = search(&mock_api(&server), &params).await.unwrap();
        assert_eq!(results.messages.len(), 2);
        assert_eq!(results.next_page_token.as_deref(), Some("more"));
        let out = search_output(&params, &results);
        assert_eq!(out["nextPageToken"], "more");
    }

    #[tokio::test]
    async fn search_fails_loudly_when_a_message_fetch_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{"id": "m1"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m1"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let params = SearchParams {
            query: None,
            max: 5,
            page_token: None,
            include_spam_trash: false,
        };
        let err = search(&mock_api(&server), &params).await.unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 404, .. }));
    }

    #[tokio::test]
    async fn search_empty_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"resultSizeEstimate": 0})),
            )
            .mount(&server)
            .await;
        let params = SearchParams {
            query: None,
            max: 5,
            page_token: None,
            include_spam_trash: false,
        };
        let results = search(&mock_api(&server), &params).await.unwrap();
        assert!(results.messages.is_empty());
    }

    #[test]
    fn summarize_extracts_metadata() {
        let s = summarize(&metadata("m9"));
        assert_eq!(s["id"], "m9");
        assert_eq!(s["threadId"], "t-m9");
        assert_eq!(s["from"], "Alice <a@example.com>");
        assert_eq!(
            s["subject"], "Subject m9",
            "header lookup is case-insensitive"
        );
        assert_eq!(s["internalDate"], "2026-01-01T00:00:00+00:00");
        assert_eq!(s["to"], "");
    }

    #[test]
    fn parse_search_args_defaults() {
        let m = helper_matches(&["+search"]);
        let p = parse_search_args(&m).unwrap();
        assert_eq!(
            p,
            SearchParams {
                query: None,
                max: 25,
                page_token: None,
                include_spam_trash: false
            }
        );
        let m = helper_matches(&[
            "+search",
            "--query",
            "is:starred",
            "--max",
            "3",
            "--page-token",
            "T",
            "--include-spam-trash",
        ]);
        let p = parse_search_args(&m).unwrap();
        assert_eq!(p.query.as_deref(), Some("is:starred"));
        assert_eq!(p.max, 3);
        assert_eq!(p.page_token.as_deref(), Some("T"));
        assert!(p.include_spam_trash);
    }
}
