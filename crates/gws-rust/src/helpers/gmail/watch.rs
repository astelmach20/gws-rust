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
    PubSubClient, ReceivedMessage, StepError, StreamOptions, StreamPrinter, StreamStep,
    cleanup_resources, emit_diagnostic, interruptible, run_stream,
};
use crate::helpers::modelarmor::{SanitizeConfig, Sanitized, sanitize_value};
use crate::transport::Transport;
use std::collections::{HashSet, VecDeque};
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
    let output_dir = crate::args::value::<String>(matches, "output-dir")?
        .map(|dir| crate::validate::validate_safe_output_dir(dir))
        .transpose()?;
    crate::helpers::events::stream::reject_stdout_flags_with_output_dir(
        matches,
        output_dir.is_some(),
    )?;
    let subscription = crate::args::value::<String>(matches, "subscription")?
        .map(|s| crate::validate::validate_resource_name(s).map(str::to_string))
        .transpose()?;
    let topic = crate::args::value::<String>(matches, "topic")?
        .map(|s| crate::validate::validate_resource_name(s).map(str::to_string))
        .transpose()?;
    let project = match crate::args::value::<String>(matches, "project")? {
        Some(p) => Some(p.clone()),
        None => crate::env::get()?.project_id.clone(),
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
        label_ids: super::cli::list_values(matches, "label-ids")?,
        max_messages: value_or_default::<u32>(matches, "max-messages")?,
        poll_interval: value_or_default::<u64>(matches, "poll-interval")?,
        max_failures: value_or_default::<u32>(matches, "max-failures")?,
        format: required_str(matches, "msg-format")?,
        once: crate::args::flag(matches, "once")?,
        cleanup: crate::args::flag(matches, "cleanup")?,
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

/// Whether a message carries at least one of the watched labels. An empty
/// filter matches everything. A message whose labels are not reported is kept
/// (never silently dropped); Gmail always reports them in history entries.
fn has_watched_label(message: &Value, label_ids: &[String]) -> bool {
    if label_ids.is_empty() {
        return true;
    }
    match message.get("labelIds").and_then(Value::as_array) {
        Some(labels) => labels
            .iter()
            .filter_map(Value::as_str)
            .any(|l| label_ids.iter().any(|w| w == l)),
        None => true,
    }
}

/// Collect message IDs added in one history page (deduplicated, in order),
/// keeping only messages with one of `label_ids` (all when empty).
///
/// `users.watch` label filters only decide which mailbox changes trigger a
/// notification; `users.history.list` since the last notification still
/// returns every added message (sent mail, drafts, other labels).
fn extract_message_ids_from_history(
    history_body: &Value,
    label_ids: &[String],
    seen: &mut HashSet<String>,
) -> Vec<String> {
    let mut result = Vec::new();
    let entries = history_body.get("history").and_then(Value::as_array);
    for entry in entries.into_iter().flatten() {
        let added = entry.get("messagesAdded").and_then(Value::as_array);
        for msg_entry in added.into_iter().flatten() {
            let Some(message) = msg_entry.get("message") else {
                continue;
            };
            if let Some(id) = message.get("id").and_then(Value::as_str)
                && has_watched_label(message, label_ids)
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
    printer: StreamPrinter,
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

    async fn output(&mut self, id: &str, msg: &Value) -> Result<(), StepError> {
        let fatal = |e: String| StepError::Fatal(GwsError::other(e));
        match &self.config.output_dir {
            Some(dir) => {
                let path = dir.join(format!("{}.json", crate::validate::encode_path_segment(id)));
                let text = serde_json::to_string_pretty(msg)
                    .map_err(|e| fatal(format!("Failed to serialize message {id}: {e}")))?;
                // The shared writer replaces (never follows) a symlink at
                // the target and applies GWSR_RESTRICT_PATHS.
                crate::output_file::write_atomic(&path, text.as_bytes(), true)
                    .await
                    .map_err(StepError::Fatal)?;
                emit_diagnostic(
                    "info",
                    "wrote",
                    json!({ "path": path.display().to_string() }),
                );
            }
            None => self.printer.print(msg)?,
        }
        Ok(())
    }

    /// Fetch, sanitize, and emit every message added since `last_history_id`.
    async fn emit_new_messages(&mut self) -> Result<(), StepError> {
        let api = self.gmail.clone();
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let mut page_token: Option<String> = None;
        let mut tokens = crate::helpers::pagination::PageTokens::new();
        loop {
            let page = api
                .history(self.last_history_id, page_token.as_deref())
                .await
                .map_err(|e| match e {
                    // Still an API error (exit 1, Google's reason and
                    // message); only the remedy is added.
                    GwsError::Api { code: 404, .. } => {
                        StepError::Fatal(crate::transport::errors::with_context(
                            e,
                            &format!(
                                "Gmail history since {} is no longer available; restart +watch",
                                self.last_history_id
                            ),
                        ))
                    }
                    other => StepError::from(other),
                })?;
            ids.extend(extract_message_ids_from_history(
                &page,
                &self.config.label_ids,
                &mut seen,
            ));
            match tokens
                .next(&page, "Failed to list Gmail history")
                .map_err(StepError::Fatal)?
            {
                Some(t) => page_token = Some(t),
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
                .map_err(StepError::Transient)?
            {
                Sanitized::Pass(v) => self.output(&id, &v).await?,
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
    async fn pull(&mut self) -> Result<Vec<ReceivedMessage>, StepError> {
        self.pubsub
            .pull(
                &self.subscription,
                self.config.max_messages,
                self.pull_timeout,
            )
            .await
    }

    async fn handle(&mut self, received: Vec<ReceivedMessage>) -> Result<(), StepError> {
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
            // A signal stops the stream before the notifications are
            // acknowledged: they announce messages not all printed yet.
            let Some(emitted) = interruptible(self.emit_new_messages()).await else {
                return Ok(());
            };
            emitted?;
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
    let mut created = Vec::new();
    match create_and_watch(config, pubsub, gmail, &mut created).await {
        Ok(started) => Ok(started),
        Err(e) => {
            // Subscription before the topic it is attached to.
            created.reverse();
            Err(roll_back(pubsub, &created, e).await)
        }
    }
}

/// Delete the Pub/Sub resources this run created when a later setup step
/// failed: nothing will ever read from or publish to them. A `--topic` the
/// user passed is never in `names`. A failed deletion is reported (naming
/// what is left behind) alongside the original error, which is returned.
async fn roll_back(pubsub: &PubSubClient, names: &[String], error: GwsError) -> GwsError {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Err(c) = cleanup_resources(pubsub, &names).await {
        emit_diagnostic(
            "error",
            "cleanup_failed",
            json!({ "message": c.to_string(), "resources": names }),
        );
    }
    error
}

/// Create the topic (unless `--topic` is given) and the subscription, then
/// start the Gmail watch. Each resource is pushed to `created` as soon as it
/// exists, so a later failure can roll it back.
async fn create_and_watch(
    config: &WatchConfig,
    pubsub: &PubSubClient,
    gmail: &GmailApi,
    created: &mut Vec<String>,
) -> Result<(String, Option<Created>, u64), GwsError> {
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
            created.push(t.clone());
            pubsub
                .set_topic_publisher(&t, GMAIL_PUSH_MEMBER)
                .await
                // Keep the error's class (a refused permission exits 1, a
                // lost connection 10); only the manual fix is added.
                .map_err(|e| crate::helpers::modelarmor::prefix_message(e, &format!(
                    "Gmail cannot publish to {t} (grant it manually: gcloud pubsub topics add-iam-policy-binding {t} --member={GMAIL_PUSH_MEMBER} --role=roles/pubsub.publisher)"
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
    created.push(sub.clone());

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

/// The command that listens to `subscription` again. `--label-ids` filters
/// what is emitted, so it is repeated: without it the reattached stream
/// would emit every added message (sent mail, drafts, other labels).
fn reconnect_command(subscription: &str, label_ids: &[String]) -> String {
    let mut command = format!("gwsr gmail +watch --subscription {subscription}");
    if !label_ids.is_empty() {
        command.push_str(" --label-ids ");
        command.push_str(&label_ids.join(","));
    }
    command
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
    if crate::args::dry_run(matches)? {
        return dry_run_plan(matches, &config);
    }
    if let Some(dir) = &config.output_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| GwsError::other(format!("Failed to create {}: {e}", dir.display())))?;
    }

    // Before any resource is created: a Ctrl+C or SIGTERM during setup must
    // not kill the process, so the stream stops at once and `--cleanup`
    // still deletes what was created.
    crate::helpers::listen_for_shutdown();
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
        printer: StreamPrinter::new(crate::helpers::http::output_format(matches)?),
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
            "command": reconnect_command(&created.subscription, &config.label_ids),
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
            extract_message_ids_from_history(&body, &[], &mut seen),
            vec!["a", "b"]
        );
        assert!(extract_message_ids_from_history(&json!({}), &[], &mut seen).is_empty());
    }

    #[test]
    fn test_extract_message_ids_from_history_filters_labels() {
        let body = json!({ "history": [
            { "messagesAdded": [
                { "message": { "id": "sent", "labelIds": ["SENT"] } },
                { "message": { "id": "inbox", "labelIds": ["UNREAD", "INBOX"] } },
                { "message": { "id": "work", "labelIds": ["Label_7"] } },
                { "message": { "id": "unknown" } }
            ] }
        ]});
        let watched = vec!["INBOX".to_string(), "Label_7".to_string()];
        assert_eq!(
            extract_message_ids_from_history(&body, &watched, &mut HashSet::new()),
            vec!["inbox", "work", "unknown"]
        );
        assert_eq!(
            extract_message_ids_from_history(&body, &[], &mut HashSet::new()),
            vec!["sent", "inbox", "work", "unknown"]
        );
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

    /// `--output-dir` writes each message as JSON to its own file, so the
    /// stdout-only flags `--jq`, `--columns` and a non-JSON `--format` would be
    /// ignored: they are refused instead (exit 3).
    #[test]
    fn test_parse_watch_args_output_dir_rejects_stdout_flags() {
        let base = ["+watch", "--subscription", "s", "--output-dir", "out"];
        for extra in [
            &["--jq", ".id"][..],
            &["--format", "table"][..],
            &["--format", "csv", "--columns", "id"][..],
            &["--format", "yaml"][..],
        ] {
            let args: Vec<&str> = base.iter().chain(extra).copied().collect();
            let Err(err) = parse_watch_args(&helper_matches(&args)) else {
                panic!("{extra:?} with --output-dir was accepted");
            };
            assert!(matches!(err, GwsError::Validation(_)), "{extra:?}: {err:?}");
            assert!(err.to_string().contains("--output-dir"), "{err}");
        }
        // JSON is what the files hold, and without --output-dir every flag applies.
        let args: Vec<&str> = base.iter().chain(&["--format", "json"]).copied().collect();
        assert!(parse_watch_args(&helper_matches(&args)).is_ok());
        let m = helper_matches(&["+watch", "--subscription", "s", "--jq", ".id"]);
        assert!(parse_watch_args(&m).is_ok());
    }

    // --- Setup rollback: a failed later step deletes what this run created ---

    const TOPIC_RE: &str = r"^/v1/projects/p/topics/gwsr-gmail-watch-[0-9a-f]{8}$";
    const SUB_RE: &str = r"^/v1/projects/p/subscriptions/gwsr-gmail-watch-[0-9a-f]{8}$";

    fn api_error(code: u16, message: &str) -> ResponseTemplate {
        ResponseTemplate::new(code).set_body_json(json!({
            "error": { "code": code, "message": message, "status": "PERMISSION_DENIED" }
        }))
    }

    /// Status of each setup call; `None` means it is never made.
    struct SetupScript {
        topic: Option<&'static str>,
        grant: Option<u16>,
        subscription: Option<u16>,
        watch: Option<u16>,
        delete: u16,
    }

    /// Run `setup` against a mock serving `script`, returning its error and
    /// the DELETE paths it sent, in order.
    async fn failed_setup(script: SetupScript) -> (GwsError, Vec<String>) {
        use wiremock::matchers::path_regex;
        let server = MockServer::start().await;
        let ok = || ResponseTemplate::new(200).set_body_json(json!({}));
        let respond = |code: u16, message: &str| match code {
            200 => ok(),
            _ => api_error(code, message),
        };
        Mock::given(method("PUT"))
            .and(path_regex(TOPIC_RE))
            .respond_with(ok())
            .expect(u64::from(script.topic.is_none()))
            .mount(&server)
            .await;
        if let Some(code) = script.grant {
            Mock::given(method("POST"))
                .and(path_regex(r":setIamPolicy$"))
                .respond_with(respond(code, "denied"))
                .expect(1)
                .mount(&server)
                .await;
        }
        if let Some(code) = script.subscription {
            Mock::given(method("PUT"))
                .and(path_regex(SUB_RE))
                .respond_with(respond(code, "denied"))
                .expect(1)
                .mount(&server)
                .await;
        }
        if let Some(code) = script.watch {
            Mock::given(method("POST"))
                .and(path("/gmail/v1/users/me/watch"))
                .respond_with(respond(code, "Invalid topicName"))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("DELETE"))
            .respond_with(respond(script.delete, "cannot delete"))
            .mount(&server)
            .await;
        let config = WatchConfig {
            project: Some("p".into()),
            subscription: None,
            topic: script.topic.map(str::to_string),
            label_ids: Vec::new(),
            max_messages: 10,
            poll_interval: 1,
            max_failures: 3,
            format: "full".into(),
            once: true,
            cleanup: false,
            output_dir: None,
        };
        let pubsub = PubSubClient::with_base(
            &Transport::for_test(&server.uri()),
            &format!("{}/v1", server.uri()),
        );
        let gmail = crate::helpers::gmail::test_support::mock_api(&server);
        let Err(err) = setup(&config, &pubsub, &gmail).await else {
            panic!("setup must fail");
        };
        server.verify().await;
        let deleted = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.method == wiremock::http::Method::DELETE)
            .map(|r| r.url.path().to_string())
            .collect();
        (err, deleted)
    }

    fn assert_deleted(deleted: &[String], patterns: &[&str]) {
        assert_eq!(deleted.len(), patterns.len(), "{deleted:?}");
        for (got, re) in deleted.iter().zip(patterns) {
            assert!(
                regex::Regex::new(re).unwrap().is_match(got),
                "{got} !~ {re}"
            );
        }
    }

    /// A refused publish grant deletes the topic just created; the grant
    /// error (with its manual fix) is still the one returned.
    #[tokio::test]
    async fn setup_rolls_back_topic_when_grant_fails() {
        let (err, deleted) = failed_setup(SetupScript {
            topic: None,
            grant: Some(403),
            subscription: None,
            watch: None,
            delete: 200,
        })
        .await;
        assert!(err.to_string().contains("add-iam-policy-binding"), "{err}");
        assert_deleted(&deleted, &[TOPIC_RE]);
    }

    /// A failed subscription create deletes the topic just created and
    /// returns the subscription error unchanged.
    #[tokio::test]
    async fn setup_rolls_back_topic_when_subscription_fails() {
        let (err, deleted) = failed_setup(SetupScript {
            topic: None,
            grant: Some(200),
            subscription: Some(403),
            watch: None,
            delete: 200,
        })
        .await;
        assert!(matches!(err, GwsError::Api { code: 403, .. }), "{err:?}");
        assert!(err.to_string().contains("denied"), "{err}");
        assert_deleted(&deleted, &[TOPIC_RE]);
    }

    /// A failed users.watch deletes the subscription, then the topic, and
    /// returns Gmail's error unchanged.
    #[tokio::test]
    async fn setup_rolls_back_subscription_and_topic_when_watch_fails() {
        let (err, deleted) = failed_setup(SetupScript {
            topic: None,
            grant: Some(200),
            subscription: Some(200),
            watch: Some(400),
            delete: 200,
        })
        .await;
        assert!(matches!(err, GwsError::Api { code: 400, .. }), "{err:?}");
        assert!(err.to_string().contains("Invalid topicName"), "{err}");
        assert_deleted(&deleted, &[SUB_RE, TOPIC_RE]);
    }

    /// A `--topic` the user passed is never deleted: only the subscription
    /// this run created is.
    #[tokio::test]
    async fn setup_rollback_keeps_a_user_topic() {
        let (err, deleted) = failed_setup(SetupScript {
            topic: Some("projects/p/topics/mine"),
            grant: None,
            subscription: Some(200),
            watch: Some(400),
            delete: 200,
        })
        .await;
        assert!(matches!(err, GwsError::Api { code: 400, .. }), "{err:?}");
        assert_deleted(&deleted, &[SUB_RE]);
    }

    /// When the rollback itself fails, the setup error is still returned
    /// (not the deletion error), after trying every created resource.
    #[tokio::test]
    async fn setup_rollback_failure_keeps_the_original_error() {
        let (err, deleted) = failed_setup(SetupScript {
            topic: None,
            grant: Some(200),
            subscription: Some(200),
            watch: Some(400),
            delete: 403,
        })
        .await;
        assert!(matches!(err, GwsError::Api { code: 400, .. }), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("Invalid topicName"), "{msg}");
        assert!(!msg.contains("cannot delete"), "{msg}");
        assert_deleted(&deleted, &[SUB_RE, TOPIC_RE]);
    }

    async fn run_once(server: &MockServer, sanitize: &SanitizeConfig) -> Result<u64, GwsError> {
        run_once_with_labels(server, sanitize, &[]).await
    }

    async fn run_once_with_labels(
        server: &MockServer,
        sanitize: &SanitizeConfig,
        label_ids: &[&str],
    ) -> Result<u64, GwsError> {
        run_once_with(server, sanitize, label_ids, None).await
    }

    async fn run_once_with(
        server: &MockServer,
        sanitize: &SanitizeConfig,
        label_ids: &[&str],
        output_dir: Option<PathBuf>,
    ) -> Result<u64, GwsError> {
        let config = WatchConfig {
            project: None,
            subscription: Some("projects/test/subscriptions/demo".into()),
            topic: None,
            label_ids: label_ids.iter().map(|l| (*l).to_string()).collect(),
            max_messages: 10,
            poll_interval: 1,
            max_failures: 3,
            format: "full".into(),
            once: true,
            cleanup: false,
            output_dir,
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
            printer: StreamPrinter::new(crate::formatter::OutputFormat::Json),
        };
        let mut opts = StreamOptions::new(true, 1, 3);
        opts.backoff_base = Duration::from_millis(1);
        opts.backoff_cap = Duration::from_millis(2);
        run_stream(&opts, &mut step)
            .await
            .map(|()| step.last_history_id)
    }

    /// A refused grant on the new topic keeps the API error's class (a
    /// permission error exits 1, not 5) and still says how to grant it.
    #[tokio::test]
    async fn topic_grant_failure_keeps_the_api_error_class() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {"code": 403, "message": "denied", "status": "PERMISSION_DENIED"}
            })))
            .mount(&server)
            .await;
        let config = WatchConfig {
            project: Some("p".into()),
            subscription: None,
            topic: None,
            label_ids: Vec::new(),
            max_messages: 10,
            poll_interval: 1,
            max_failures: 3,
            format: "full".into(),
            once: true,
            cleanup: false,
            output_dir: None,
        };
        let pubsub = PubSubClient::with_base(
            &Transport::for_test(&server.uri()),
            &format!("{}/v1", server.uri()),
        );
        let gmail = crate::helpers::gmail::test_support::mock_api(&server);
        let Err(err) = setup(&config, &pubsub, &gmail).await else {
            panic!("setup must fail");
        };
        assert!(matches!(err, GwsError::Api { code: 403, .. }), "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("denied"), "{msg}");
        assert!(
            msg.contains("gcloud pubsub topics add-iam-policy-binding"),
            "{msg}"
        );
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

    /// `--output-dir` files are written through the shared output writer: a
    /// symlink planted at a message's file name is replaced, never followed
    /// to clobber the file it points at.
    #[cfg(unix)]
    #[tokio::test]
    async fn output_dir_does_not_follow_a_planted_symlink() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [{ "messagesAdded": [{ "message": { "id": "msg-1" } }] }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/messages/msg-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "msg-1" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let victim = root.path().join("victim.txt");
        std::fs::write(&victim, "keep me").unwrap();
        std::os::unix::fs::symlink(&victim, out.join("msg-1.json")).unwrap();

        run_once_with(&server, &SanitizeConfig::default(), &[], Some(out.clone()))
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
        let written = out.join("msg-1.json");
        assert!(!written.symlink_metadata().unwrap().file_type().is_symlink());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&written).unwrap()).unwrap();
        assert_eq!(v["id"], "msg-1");
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

    /// History older than Gmail keeps is a fatal API error (exit 1) with
    /// Google's reason and message, plus the remedy; not an internal error.
    #[tokio::test]
    async fn watch_history_gone_is_fatal() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": {
                    "code": 404,
                    "message": "Requested entity was not found.",
                    "errors": [{ "reason": "notFound" }]
                }
            })))
            .expect(1)
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
        assert!(
            matches!(&err, GwsError::Api { code: 404, reason, .. } if reason == "notFound"),
            "{err:?}"
        );
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
        let msg = err.to_string();
        for needle in [
            "Requested entity was not found.",
            "since 1",
            "restart +watch",
        ] {
            assert!(msg.contains(needle), "{needle}: {msg}");
        }
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

    /// History paging treats an empty nextPageToken as the last page and a
    /// repeated one as a fatal error, never paging forever.
    #[tokio::test]
    async fn watch_history_stops_on_repeated_or_empty_page_token() {
        let limit = Duration::from_secs(10);
        for (token, ok) in [("same", false), ("", true)] {
            let server = MockServer::start().await;
            mount_pull(&server).await;
            Mock::given(method("GET"))
                .and(path("/gmail/v1/users/me/history"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "historyId": "2",
                    "nextPageToken": token
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;
            let res = tokio::time::timeout(limit, run_once(&server, &SanitizeConfig::default()))
                .await
                .unwrap_or_else(|_| panic!("nextPageToken {token:?} must end pagination"));
            if ok {
                res.unwrap();
            } else {
                let err = res.unwrap_err();
                assert!(err.to_string().contains("twice"), "{err}");
            }
        }
    }

    /// The reconnect hint keeps `--label-ids`: the labels filter what the
    /// reattached stream emits.
    #[test]
    fn reconnect_command_repeats_the_label_filter() {
        let sub = "projects/p/subscriptions/s";
        assert_eq!(
            reconnect_command(sub, &[]),
            "gwsr gmail +watch --subscription projects/p/subscriptions/s"
        );
        assert_eq!(
            reconnect_command(sub, &["INBOX".to_string(), "UNREAD".to_string()]),
            "gwsr gmail +watch --subscription projects/p/subscriptions/s --label-ids INBOX,UNREAD"
        );
    }

    /// `--label-ids` must filter what is emitted, not only which changes
    /// trigger a notification: history since the last notification also
    /// contains messages outside the watched labels (sent mail, drafts).
    #[tokio::test]
    async fn watch_emits_only_messages_with_watched_labels() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "history": [
                    { "messagesAdded": [{ "message": { "id": "sent-1", "labelIds": ["SENT"] } }] },
                    { "messagesAdded": [{ "message": { "id": "draft-1", "labelIds": ["DRAFT"] } }] },
                    { "messagesAdded": [{ "message": { "id": "in-1", "labelIds": ["UNREAD", "INBOX"] } }] }
                ]
            })))
            .mount(&server)
            .await;
        for (id, n) in [("in-1", 1), ("sent-1", 0), ("draft-1", 0)] {
            Mock::given(method("GET"))
                .and(path(format!("/gmail/v1/users/me/messages/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": id })))
                .expect(n)
                .named(id)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        run_once_with_labels(&server, &SanitizeConfig::default(), &["INBOX"])
            .await
            .unwrap();
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
