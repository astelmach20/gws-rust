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

//! `gmail +label`, `+archive`, `+trash`: change labels on messages or threads.

use super::api::resolve_label_ids;
use super::cli::list_values;
use super::prelude::*;
use super::resolve_url::thread_id_from_input;

/// The messages and threads a command acts on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Targets {
    pub messages: Vec<String>,
    pub threads: Vec<String>,
}

impl Targets {
    pub(super) fn from_matches(matches: &ArgMatches) -> Result<Self, GwsError> {
        let messages = list_values(matches, "message-id")?;
        let threads = list_values(matches, "thread-id")?
            .iter()
            .map(|t| thread_id_from_input(t))
            .collect::<Result<Vec<_>, _>>()?;
        if messages.is_empty() && threads.is_empty() {
            return Err(GwsError::Validation(
                "Provide at least one --message-id or --thread-id".to_string(),
            ));
        }
        Ok(Self { messages, threads })
    }
}

/// A planned label change, before label names are resolved to IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LabelChange {
    add: Vec<String>,
    remove: Vec<String>,
}

/// Apply label changes (already resolved to IDs) and return a summary.
async fn apply(
    api: &GmailApi,
    targets: &Targets,
    add: &[String],
    remove: &[String],
) -> Result<Value, GwsError> {
    if !targets.messages.is_empty() {
        api.modify_messages(&targets.messages, add, remove).await?;
    }
    for thread in &targets.threads {
        api.modify_thread(thread, add, remove).await?;
    }
    Ok(json!({
        "messages": targets.messages,
        "threads": targets.threads,
        "addedLabelIds": add,
        "removedLabelIds": remove,
    }))
}

/// Requests that `apply` would make, for `--dry-run`.
fn plan(api: &GmailApi, targets: &Targets, add: &[String], remove: &[String]) -> Vec<Value> {
    let mut requests = Vec::new();
    if !targets.messages.is_empty() {
        let (url, body) = api.modify_messages_request(&targets.messages, add, remove);
        requests.push(crate::helpers::http::dry_run_request(
            "POST",
            &url,
            &[],
            Some(&body),
        ));
    }
    for thread in &targets.threads {
        let body = json!({ "addLabelIds": add, "removeLabelIds": remove });
        requests.push(crate::helpers::http::dry_run_request(
            "POST",
            &api.modify_thread_url(thread),
            &[],
            Some(&body),
        ));
    }
    requests
}

async fn print(
    value: Value,
    matches: &ArgMatches,
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let format = crate::helpers::http::output_format(matches)?;
    super::emit_screened(sanitize, &format, value).await
}

/// An unauthenticated client used only to render dry-run URLs.
fn offline_api() -> Result<GmailApi, GwsError> {
    Ok(GmailApi::new(
        crate::transport::Transport::unauthenticated()?
    ))
}

/// Handle `+label`.
pub(super) async fn handle_label(
    matches: &ArgMatches,
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let targets = Targets::from_matches(matches)?;
    let change = LabelChange {
        add: list_values(matches, "add")?,
        remove: list_values(matches, "remove")?,
    };
    if change.add.is_empty() && change.remove.is_empty() {
        return Err(GwsError::Validation(
            "Provide --add and/or --remove".to_string(),
        ));
    }
    if crate::args::dry_run(matches)? {
        // Label names are resolved to IDs at run time; the plan shows them as given.
        return crate::helpers::http::print_dry_run(
            matches,
            plan(&offline_api()?, &targets, &change.add, &change.remove),
        );
    }
    let api = super::api::authenticated(&[GMAIL_SCOPE]).await?;
    let labels = api.list_labels().await?;
    let add = resolve_label_ids(&change.add, &labels)?;
    let remove = resolve_label_ids(&change.remove, &labels)?;
    let summary = apply(&api, &targets, &add, &remove).await?;
    print(summary, matches, sanitize).await
}

/// Handle `+archive` (remove the INBOX label).
pub(super) async fn handle_archive(
    matches: &ArgMatches,
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let targets = Targets::from_matches(matches)?;
    let remove = vec!["INBOX".to_string()];
    if crate::args::dry_run(matches)? {
        return crate::helpers::http::print_dry_run(
            matches,
            plan(&offline_api()?, &targets, &[], &remove),
        );
    }
    let api = super::api::authenticated(&[GMAIL_SCOPE]).await?;
    let summary = apply(&api, &targets, &[], &remove).await?;
    print(summary, matches, sanitize).await
}

/// Handle `+trash`.
pub(super) async fn handle_trash(
    matches: &ArgMatches,
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let targets = Targets::from_matches(matches)?;
    let items: Vec<(TargetKind, &String)> = targets
        .messages
        .iter()
        .map(|m| (TargetKind::Message, m))
        .chain(targets.threads.iter().map(|t| (TargetKind::Thread, t)))
        .collect();
    if crate::args::dry_run(matches)? {
        let api = offline_api()?;
        let requests = items
            .iter()
            .map(|(kind, id)| {
                crate::helpers::http::dry_run_request("POST", &api.trash_url(*kind, id), &[], None)
            })
            .collect();
        return crate::helpers::http::print_dry_run(matches, requests);
    }
    let api = super::api::authenticated(&[GMAIL_SCOPE]).await?;
    for (kind, id) in &items {
        api.trash(*kind, id).await?;
    }
    print(
        json!({ "trashedMessages": targets.messages, "trashedThreads": targets.threads }),
        matches,
        sanitize,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::api::Label;
    use crate::helpers::gmail::test_support::{helper_matches, mock_api};
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn targets_resolve_thread_urls() {
        let m = helper_matches(&[
            "+archive",
            "--message-id",
            "a,b",
            "--thread-id",
            "https://mail.google.com/mail/u/0/#inbox/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW",
        ]);
        let t = Targets::from_matches(&m).unwrap();
        assert_eq!(t.messages, vec!["a", "b"]);
        assert_eq!(t.threads, vec!["19de9c93ce0566d3"]);
    }

    #[tokio::test]
    async fn apply_uses_batch_modify_and_thread_modify() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/batchModify"))
            .and(body_json(
                json!({"ids": ["a", "b"], "addLabelIds": ["L1"], "removeLabelIds": []}),
            ))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/threads/t1/modify"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "t1"})))
            .expect(1)
            .mount(&server)
            .await;
        let targets = Targets {
            messages: vec!["a".into(), "b".into()],
            threads: vec!["t1".into()],
        };
        let out = apply(&mock_api(&server), &targets, &["L1".into()], &[])
            .await
            .unwrap();
        assert_eq!(out["addedLabelIds"][0], "L1");
    }

    #[tokio::test]
    async fn apply_propagates_failures() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let targets = Targets {
            messages: vec!["a".into()],
            threads: vec![],
        };
        let err = apply(&mock_api(&server), &targets, &[], &["INBOX".into()])
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 404, .. }));
    }

    #[test]
    fn label_names_resolve_or_fail() {
        let labels = vec![Label {
            id: "Label_9".into(),
            name: "Receipts".into(),
        }];
        assert_eq!(
            resolve_label_ids(&["Receipts".into()], &labels).unwrap(),
            vec!["Label_9"]
        );
        assert!(resolve_label_ids(&["Missing".into()], &labels).is_err());
    }

    #[test]
    fn dry_run_plan_lists_every_request() {
        let targets = Targets {
            messages: vec!["m1".into()],
            threads: vec!["t1".into(), "t2".into()],
        };
        let reqs = plan(&offline_api().unwrap(), &targets, &[], &["INBOX".into()]);
        assert_eq!(reqs.len(), 3);
        assert!(
            reqs[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/users/me/messages/m1/modify")
        );
        assert!(
            reqs[2]["url"]
                .as_str()
                .unwrap()
                .ends_with("/users/me/threads/t2/modify")
        );
    }
}
