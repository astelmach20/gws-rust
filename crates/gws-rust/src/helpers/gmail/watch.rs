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

//! `gmail +watch`: stream new messages as NDJSON via Gmail push + Pub/Sub.
//!
//! Delivery is at-least-once: Pub/Sub notifications are acknowledged only
//! after every new message they announce has been emitted, so a failure
//! mid-batch leads to redelivery rather than loss. Messages already emitted in
//! this session are not emitted twice.

use super::cli::{required_str, value_or_default};
use super::prelude::*;
use crate::helpers::events::stream::{
    PubSubClient, StepError, StreamOptions, StreamStep, cleanup_resources, emit_diagnostic,
    run_stream,
};
use crate::helpers::modelarmor::{SanitizeConfig, Sanitized, sanitize_value};
use crate::transport::Transport;
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Gmail's push service account that must be able to publish to the topic.
const GMAIL_PUSH_MEMBER: &str = "serviceAccount:gmail-api-push@system.gserviceaccount.com";
/// Remember this many emitted message IDs to suppress duplicates.
const EMITTED_MEMORY: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
struct WatchConfig {
    project: Option<String>,
    subscription: Option<String>,
    topic: Option<String>,
    label_ids: Vec<String>,
    max_messages: u32,
    poll_interval: u64,
    max_failures: u32,
    format: String,
    once: bool,
    cleanup: bool,
    output_dir: Option<PathBuf>,
}

fn parse_watch_args(matches: &ArgMatches) -> Result<WatchConfig, GwsError> {
    let output_dir = matches
        .get_one::<String>("output-dir")
        .map(|dir| crate::validate::validate_safe_output_dir(dir))
        .transpose()?;
    let subscription = matches
        .get_one::<String>("subscription")
        .map(|s| crate::validate::validate_resource_name(s).map(str::to_string))
        .transpose()?;
    let topic = matches
        .get_one::<String>("topic")
        .map(|s| crate::validate::validate_resource_name(s).map(str::to_string))
        .transpose()?;
    let project = match matches.get_one::<String>("project") {
        Some(p) => Some(p.clone()),
        None => match std::env::var("GWSR_PROJECT_ID") {
            Ok(p) => Some(p),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(GwsError::Validation(
                    "GWSR_PROJECT_ID is not valid UTF-8".to_string(),
                ));
            }
        },
    }
    .map(|p| crate::validate::validate_resource_name(&p).map(str::to_string))
    .transpose()?;
    if subscription.is_none() && project.is_none() {
        return Err(GwsError::Validation(
            "--project is required when not using --subscription (or set GWSR_PROJECT_ID)"
                .to_string(),
        ));
    }
    Ok(WatchConfig {
        project,
        subscription,
        topic,
        label_ids: super::cli::list_values(matches, "label-ids"),
        max_messages: value_or_default::<u32>(matches, "max-messages")?,
        poll_interval: value_or_default::<u64>(matches, "poll-interval")?,
        max_failures: value_or_default::<u32>(matches, "max-failures")?,
        format: required_str(matches, "msg-format")?,
        once: matches.get_flag("once"),
        cleanup: matches.get_flag("cleanup"),
        output_dir,
    })
}

/// Extract the Gmail `historyId` from a Pub/Sub notification message.
fn notification_history_id(message: &Value) -> Result<u64, String> {
    let data = message
        .get("data")
        .and_then(Value::as_str)
        .ok_or("notification has no data")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("notification data is not base64: {e}"))?;
    let json: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("notification data is not JSON: {e}"))?;
    let h = json
        .get("historyId")
        .ok_or("notification has no historyId")?;
    h.as_u64()
        .or_else(|| h.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| format!("invalid historyId {h}"))
}

/// Parse a `historyId` field (string or number) from an API response.
fn parse_history_id(v: &Value, context: &str) -> Result<u64, GwsError> {
    let h = v
        .get("historyId")
        .ok_or_else(|| GwsError::other(format!("{context}: response has no historyId")))?;
    h.as_u64()
        .or_else(|| h.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| GwsError::other(format!("{context}: invalid historyId {h}")))
}

/// Collect message IDs added in one history page (deduplicated, in order).
fn extract_message_ids_from_history(
    history_body: &Value,
    seen: &mut HashSet<String>,
) -> Vec<String> {
    let mut result = Vec::new();
    let entries = history_body.get("history").and_then(Value::as_array);
    for entry in entries.into_iter().flatten() {
        let added = entry.get("messagesAdded").and_then(Value::as_array);
        for msg_entry in added.into_iter().flatten() {
            if let Some(id) = msg_entry
                .get("message")
                .and_then(|m| m.get("id"))
                .and_then(Value::as_str)
                && seen.insert(id.to_string())
            {
                result.push(id.to_string());
            }
        }
    }
    result
}

/// State for one `+watch` session.
struct GmailWatchStep<'a> {
    gmail: GmailApi,
    pubsub: PubSubClient,
    subscription: String,
    config: WatchConfig,
    sanitize: &'a SanitizeConfig,
    last_history_id: u64,
    emitted: HashSet<String>,
    emitted_order: VecDeque<String>,
    pull_timeout: Duration,
}

impl GmailWatchStep<'_> {
    fn remember(&mut self, id: &str) {
        if self.emitted.insert(id.to_string()) {
            self.emitted_order.push_back(id.to_string());
            if self.emitted_order.len() > EMITTED_MEMORY
                && let Some(old) = self.emitted_order.pop_front()
            {
                self.emitted.remove(&old);
            }
        }
    }

    fn output(&self, id: &str, msg: &Value) -> Result<(), StepError> {
        let fatal = |e: String| StepError::Fatal(GwsError::other(e));
        match &self.config.output_dir {
            Some(dir) => {
                let path = dir.join(format!("{}.json", crate::validate::encode_path_segment(id)));
                let text = serde_json::to_string_pretty(msg)
                    .map_err(|e| fatal(format!("Failed to serialize message {id}: {e}")))?;
                std::fs::write(&path, text)
                    .map_err(|e| fatal(format!("Failed to write {}: {e}", path.display())))?;
                emit_diagnostic(
                    "info",
                    "wrote",
                    json!({ "path": path.display().to_string() }),
                );
            }
            None => {
                let line = serde_json::to_string(msg)
                    .map_err(|e| fatal(format!("Failed to serialize message {id}: {e}")))?;
                let mut out = std::io::stdout().lock();
                writeln!(out, "{line}")
                    .and_then(|()| out.flush())
                    .map_err(|e| fatal(format!("Failed to write to stdout: {e}")))?;
            }
        }
        Ok(())
    }

    /// Fetch, sanitize, and emit every message added since `last_history_id`.
    async fn emit_new_messages(&mut self) -> Result<(), StepError> {
        let api = self.gmail.clone();
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let mut page_token: Option<String> = None;
        loop {
            let page = api
                .history(self.last_history_id, page_token.as_deref())
                .await
                .map_err(|e| match e {
                    GwsError::Api { code: 404, .. } => StepError::Fatal(GwsError::other(format!(
                        "Gmail history since {} is no longer available ({e}); restart +watch",
                        self.last_history_id
                    ))),
                    other => StepError::from(other),
                })?;
            ids.extend(extract_message_ids_from_history(&page, &mut seen));
            match page.get("nextPageToken").and_then(Value::as_str) {
                Some(t) => page_token = Some(t.to_string()),
                None => break,
            }
        }

        for id in ids {
            if self.emitted.contains(&id) {
                continue;
            }
            let msg = match api.get_message(&id, &self.config.format, &[]).await {
                Ok(m) => m,
                Err(GwsError::Api { code: 404, .. }) => {
                    emit_diagnostic("warning", "message_gone", json!({ "id": id }));
                    continue;
                }
                Err(e) => return Err(StepError::from(e)),
            };
            // Fail closed: a sanitization error in block mode aborts this
            // iteration before anything is emitted or acknowledged.
            match sanitize_value(self.sanitize, msg)
                .await
                .map_err(|e| StepError::Transient {
                    status: None,
                    message: e.to_string(),
                })? {
                Sanitized::Pass(v) => self.output(&id, &v)?,
                Sanitized::Blocked(result) => emit_diagnostic(
                    "warning",
                    "blocked",
                    json!({ "id": id, "filterMatchState": result.filter_match_state }),
                ),
            }
            self.remember(&id);
        }
        Ok(())
    }
}

impl StreamStep for GmailWatchStep<'_> {
    async fn step(&mut self) -> Result<(), StepError> {
        let received = self
            .pubsub
            .pull(
                &self.subscription,
                self.config.max_messages,
                self.pull_timeout,
            )
            .await?;
        if received.is_empty() {
            return Ok(());
        }
        let mut max_history_id = 0;
        for m in &received {
            match notification_history_id(&m.message) {
                Ok(h) => max_history_id = max_history_id.max(h),
                // Nothing can be done with an undecodable notification; it is
                // reported and acknowledged so it is not redelivered forever.
                Err(reason) => emit_diagnostic(
                    "warning",
                    "undecodable_notification",
                    json!({ "reason": reason }),
                ),
            }
        }
        if max_history_id > self.last_history_id {
            self.emit_new_messages().await?;
            self.last_history_id = max_history_id;
        }
        let ack_ids: Vec<String> = received.into_iter().map(|m| m.ack_id).collect();
        self.pubsub.ack(&self.subscription, &ack_ids).await
    }
}

/// Pub/Sub resources created for this session.
struct Created {
    topic: Option<String>,
    subscription: String,
}

/// Create the topic/subscription (unless given) and start the Gmail watch.
/// Returns the subscription, created resources, and the starting historyId.
async fn setup(
    config: &WatchConfig,
    pubsub: &PubSubClient,
    gmail: &GmailApi,
) -> Result<(String, Option<Created>, u64), GwsError> {
    if let Some(sub) = &config.subscription {
        let profile = gmail.profile().await?;
        return Ok((
            sub.clone(),
            None,
            parse_history_id(&profile, "Gmail profile")?,
        ));
    }
    let project = config
        .project
        .as_deref()
        .ok_or_else(|| GwsError::Validation("--project is required".to_string()))?;
    let suffix = format!("{:08x}", rand::random::<u32>());
    let (topic, created_topic) = match &config.topic {
        Some(t) => (t.clone(), None),
        None => {
            let t = format!("projects/{project}/topics/gwsr-gmail-watch-{suffix}");
            emit_diagnostic("info", "creating_topic", json!({ "topic": t }));
            pubsub.create_topic(&t).await?;
            pubsub
                .set_topic_publisher(&t, GMAIL_PUSH_MEMBER)
                .await
                .map_err(|e| GwsError::other(format!(
                    "{e}. Grant it manually: gcloud pubsub topics add-iam-policy-binding {t} --member={GMAIL_PUSH_MEMBER} --role=roles/pubsub.publisher"
                )))?;
            (t.clone(), Some(t))
        }
    };
    let sub = format!("projects/{project}/subscriptions/gwsr-gmail-watch-{suffix}");
    emit_diagnostic(
        "info",
        "creating_subscription",
        json!({ "subscription": sub }),
    );
    pubsub.create_subscription(&sub, &topic).await?;

    let mut watch_body = json!({ "topicName": topic });
    if !config.label_ids.is_empty() {
        watch_body["labelIds"] = json!(config.label_ids);
    }
    let watch = gmail.watch(&watch_body).await?;
    let history_id = parse_history_id(&watch, "gmail.users.watch")?;
    emit_diagnostic(
        "info",
        "watch_active",
        json!({ "historyId": history_id, "expiration": watch.get("expiration") }),
    );
    Ok((
        sub.clone(),
        Some(Created {
            topic: created_topic,
            subscription: sub,
        }),
        history_id,
    ))
}

fn dry_run_plan(matches: &ArgMatches, config: &WatchConfig) -> Result<(), GwsError> {
    let plan = json!({
        "dry_run": true,
        "action": if config.subscription.is_some() { "listen to existing subscription" } else { "create Pub/Sub topic + subscription and start gmail.users.watch" },
        "subscription": config.subscription,
        "project": config.project,
        "topic": config.topic,
        "labelIds": config.label_ids,
    });
    crate::helpers::http::print_value(matches, &plan)
}

/// Handles the `+watch` command.
pub(super) async fn handle_watch(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    let config = parse_watch_args(matches)?;
    if crate::helpers::http::dry_run(matches) {
        return dry_run_plan(matches, &config);
    }
    if let Some(dir) = &config.output_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| GwsError::other(format!("Failed to create {}: {e}", dir.display())))?;
    }

    let pubsub = PubSubClient::new(&Transport::for_scopes(&[PUBSUB_SCOPE]).await?);
    let gmail = GmailApi::new(Transport::for_scopes(&[GMAIL_SCOPE]).await?);

    let (subscription, created, history_id) = setup(&config, &pubsub, &gmail).await?;

    let opts = StreamOptions::new(config.once, config.poll_interval, config.max_failures);
    let mut step = GmailWatchStep {
        gmail,
        pubsub: pubsub.clone(),
        subscription: subscription.clone(),
        pull_timeout: Duration::from_secs(config.poll_interval.max(10)),
        config: config.clone(),
        sanitize: sanitize_config,
        last_history_id: history_id,
        emitted: HashSet::new(),
        emitted_order: VecDeque::new(),
    };
    let result = run_stream(&opts, &mut step).await;

    let Some(created) = created else {
        return result;
    };
    if config.cleanup {
        let mut names = vec![created.subscription.as_str()];
        if let Some(t) = &created.topic {
            names.push(t);
        }
        let cleaned = cleanup_resources(&pubsub, &names).await;
        return match (result, cleaned) {
            (Err(e), Err(c)) => {
                emit_diagnostic(
                    "error",
                    "cleanup_failed",
                    json!({ "message": c.to_string() }),
                );
                Err(e)
            }
            (Err(e), Ok(())) => Err(e),
            (Ok(()), c) => c,
        };
    }
    emit_diagnostic(
        "info",
        "reconnect",
        json!({
            "subscription": created.subscription,
            "topic": created.topic,
            "command": format!("gwsr gmail +watch --subscription {}", created.subscription),
            "note": "Gmail watch expires after 7 days; re-run +watch to renew",
        }),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::helper_matches;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn notification(history_id: u64) -> String {
        base64::engine::general_purpose::STANDARD
            .encode(json!({ "historyId": history_id }).to_string())
    }

    #[test]
    fn test_extract_message_ids_from_history_dedupes() {
        let body = json!({ "history": [
            { "messagesAdded": [ { "message": { "id": "a" } }, { "message": { "id": "b" } } ] },
            { "messagesAdded": [ { "message": { "id": "a" } } ] },
            { "labelsAdded": [] }
        ]});
        let mut seen = HashSet::new();
        assert_eq!(
            extract_message_ids_from_history(&body, &mut seen),
            vec!["a", "b"]
        );
        assert!(extract_message_ids_from_history(&json!({}), &mut seen).is_empty());
    }

    #[test]
    fn test_notification_history_id() {
        assert_eq!(
            notification_history_id(&json!({ "data": notification(42) })).unwrap(),
            42
        );
        let s = base64::engine::general_purpose::STANDARD.encode(r#"{"historyId":"7"}"#);
        assert_eq!(notification_history_id(&json!({ "data": s })).unwrap(), 7);
        assert!(notification_history_id(&json!({})).is_err());
        assert!(notification_history_id(&json!({ "data": "!!" })).is_err());
    }

    #[test]
    fn test_parse_history_id_requires_value() {
        assert_eq!(
            parse_history_id(&json!({"historyId": "123"}), "x").unwrap(),
            123
        );
        assert!(parse_history_id(&json!({}), "x").is_err());
    }

    #[test]
    fn test_parse_watch_args_full() {
        let m = helper_matches(&[
            "+watch",
            "--project",
            "p",
            "--label-ids",
            "INBOX, UNREAD",
            "--max-messages",
            "5",
            "--poll-interval",
            "3",
            "--max-failures",
            "4",
            "--msg-format",
            "metadata",
            "--once",
            "--cleanup",
        ]);
        let c = parse_watch_args(&m).unwrap();
        assert_eq!(c.project.as_deref(), Some("p"));
        assert_eq!(c.label_ids, vec!["INBOX", "UNREAD"]);
        assert_eq!((c.max_messages, c.poll_interval, c.max_failures), (5, 3, 4));
        assert_eq!(c.format, "metadata");
        assert!(c.once && c.cleanup);
    }

    #[test]
    fn test_parse_watch_args_validation() {
        let m = helper_matches(&["+watch", "--subscription", "projects/p/subscriptions/../x"]);
        assert!(parse_watch_args(&m).is_err());
        let m = helper_matches(&[
            "+watch",
            "--subscription",
            "s",
            "--output-dir",
            "bad\x01dir",
        ]);
        assert!(parse_watch_args(&m).is_err());
        let m = helper_matches(&["+watch", "--subscription", "projects/p/subscriptions/s"]);
        let c = parse_watch_args(&m).unwrap();
        assert_eq!(
            (c.max_messages, c.poll_interval, c.max_failures),
            (10, 5, 10)
        );
        assert_eq!(c.format, "full");
    }

    async fn run_once(server: &MockServer, sanitize: &SanitizeConfig) -> Result<u64, GwsError> {
        let config = WatchConfig {
            project: None,
            subscription: Some("projects/test/subscriptions/demo".into()),
            topic: None,
            label_ids: vec![],
            max_messages: 10,
            poll_interval: 1,
            max_failures: 3,
            format: "full".into(),
            once: true,
            cleanup: false,
            output_dir: None,
        };
        let mut step = GmailWatchStep {
            gmail: crate::helpers::gmail::test_support::mock_api(server),
            pubsub: PubSubClient::with_base(
                &Transport::for_test(&server.uri()),
                &format!("{}/v1", server.uri()),
            ),
            subscription: "projects/test/subscriptions/demo".into(),
            pull_timeout: Duration::from_secs(5),
            config,
            sanitize,
            last_history_id: 1,
            emitted: HashSet::new(),
            emitted_order: VecDeque::new(),
        };
        let mut opts = StreamOptions::new(true, 1, 3);
        opts.backoff_base = Duration::from_millis(1);
        opts.backoff_cap = Duration::from_millis(2);
        run_stream(&opts, &mut step)
            .await
            .map(|()| step.last_history_id)
    }

    async fn mount_pull(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "receivedMessages": [{ "ackId": "ack-1", "message": { "data": notification(2) } }]
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn watch_pulls_fetches_emits_and_acks() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .and(query_param("startHistoryId", "1"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "msg-1" } }] }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/msg-1"))
            .and(query_param("format", "full"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "msg-1" })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let last = run_once(&server, &SanitizeConfig::default()).await.unwrap();
        assert_eq!(last, 2);
    }

    #[tokio::test]
    async fn watch_retries_transient_pull_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        run_once(&server, &SanitizeConfig::default()).await.unwrap();
    }

    #[tokio::test]
    async fn watch_does_not_ack_when_message_fetch_fails() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "m" } }] }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let err = run_once(&server, &SanitizeConfig::default())
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 403, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn watch_history_gone_is_fatal() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = run_once(&server, &SanitizeConfig::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("restart +watch"));
    }

    #[tokio::test]
    async fn watch_paginates_history() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "m2" } }] }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "m1" } }] }],
                "nextPageToken": "p2"
            })))
            .mount(&server)
            .await;
        for id in ["m1", "m2"] {
            Mock::given(method("GET"))
                .and(path(format!("/gmail/v1/users/me/messages/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": id })))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        run_once(&server, &SanitizeConfig::default()).await.unwrap();
    }

    /// SEC-14: when block-mode sanitization cannot run, nothing is emitted or acked.
    #[tokio::test]
    async fn watch_block_mode_sanitization_failure_fails_closed() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "m" } }] }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/m"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "m" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        // An invalid template makes sanitization fail without network access.
        let sanitize = SanitizeConfig {
            template: Some("projects/p/locations/evil.com#/templates/t".into()),
            mode: crate::helpers::modelarmor::SanitizeMode::Block,
        };
        let err = run_once(&server, &sanitize).await.unwrap_err();
        assert!(err.to_string().contains("giving up"), "{err}");
    }
}
