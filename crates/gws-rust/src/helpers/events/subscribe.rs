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

//! `events +subscribe`: create (or reuse) a Pub/Sub-backed Workspace Events
//! subscription and stream CloudEvents as NDJSON.

use super::stream::{
    PubSubClient, StepError, StreamOptions, StreamStep, cleanup_resources, emit_diagnostic,
    run_stream,
};
use super::{PUBSUB_SCOPE, WORKSPACE_EVENTS_API_BASE, parse_event_types, scopes_for_event_types};

use crate::error::GwsError;
use crate::helpers::modelarmor::{SanitizeConfig, Sanitized, sanitize_value};
use crate::transport::Transport;
use base64::Engine as _;
use clap::ArgMatches;
use gws_rust_core::client::Idempotency;
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SubscribeConfig {
    target: Option<String>,
    event_types: Vec<String>,
    project: Option<String>,
    subscription: Option<String>,
    max_messages: u32,
    poll_interval: u64,
    max_failures: u32,
    once: bool,
    cleanup: bool,
    no_ack: bool,
    output_dir: Option<PathBuf>,
}

fn typed<T: Clone + Send + Sync + 'static>(
    matches: &ArgMatches,
    name: &str,
) -> Result<T, GwsError> {
    crate::args::defaulted(matches, name)
}

fn parse_subscribe_args(matches: &ArgMatches) -> Result<SubscribeConfig, GwsError> {
    let project = match crate::args::value::<String>(matches, "project")? {
        Some(p) => Some(p.clone()),
        None => crate::env::get()?.project_id.clone(),
    };
    let config = SubscribeConfig {
        target: crate::args::value::<String>(matches, "target")?
            .map(|t| validate_target(t))
            .transpose()?,
        event_types: parse_event_types(crate::args::value::<String>(matches, "event-types")?),
        project: project
            .map(|p| crate::validate::validate_resource_name(&p).map(str::to_string))
            .transpose()?,
        subscription: crate::args::value::<String>(matches, "subscription")?
            .map(|s| crate::validate::validate_resource_name(s).map(str::to_string))
            .transpose()?,
        max_messages: typed::<u32>(matches, "max-messages")?,
        poll_interval: typed::<u64>(matches, "poll-interval")?,
        max_failures: typed::<u32>(matches, "max-failures")?,
        once: crate::args::flag(matches, "once")?,
        cleanup: crate::args::flag(matches, "cleanup")?,
        no_ack: crate::args::flag(matches, "no-ack")?,
        output_dir: crate::args::value::<String>(matches, "output-dir")?
            .map(|d| crate::validate::validate_safe_output_dir(d))
            .transpose()?,
    };
    validate_subscribe_config(&config)?;
    Ok(config)
}

/// Validate a Workspace Events target: a full resource name such as
/// `//chat.googleapis.com/spaces/AAAA`. The leading `//` (service host) is
/// part of the format; everything after it must be a clean resource name.
fn validate_target(target: &str) -> Result<String, GwsError> {
    let rest = target.strip_prefix("//").ok_or_else(|| {
        GwsError::Validation(format!(
            "--target must be a full resource name starting with '//' \
             (e.g. //chat.googleapis.com/spaces/SPACE), got '{target}'"
        ))
    })?;
    crate::validate::validate_resource_name(rest)?;
    Ok(target.to_string())
}

fn validate_subscribe_config(config: &SubscribeConfig) -> Result<(), GwsError> {
    if config.subscription.is_none() {
        if config.target.is_none() {
            return Err(GwsError::Validation(
                "--target is required when not using --subscription".to_string(),
            ));
        }
        if config.event_types.is_empty() {
            return Err(GwsError::Validation(
                "--event-types is required when not using --subscription".to_string(),
            ));
        }
        if config.project.is_none() {
            return Err(GwsError::Validation(
                "--project is required when not using --subscription (or set GWSR_PROJECT_ID)"
                    .to_string(),
            ));
        }
        scopes_for_event_types(&config.event_types)?;
    }
    Ok(())
}

/// Decode a Pub/Sub message carrying a CloudEvent. Undecodable data is kept
/// as `null` with an explicit `dataError` rather than silently dropped.
fn decode_cloud_event(pubsub_msg: &Value) -> Value {
    let attributes = pubsub_msg
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let attr = |k: &str| {
        attributes
            .get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let mut event = json!({
        "type": attr("type"),
        "source": attr("source"),
        "time": attr("time"),
        "attributes": attributes,
        "data": Value::Null,
    });
    match pubsub_msg.get("data").and_then(Value::as_str) {
        None => {}
        Some(d) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(d)
                .map_err(|e| format!("data is not base64: {e}"))
                .and_then(|bytes| {
                    serde_json::from_slice::<Value>(&bytes)
                        .map_err(|e| format!("data is not JSON: {e}"))
                });
            match decoded {
                Ok(v) => event["data"] = v,
                Err(msg) => event["dataError"] = json!(msg),
            }
        }
    }
    event
}

/// Derives a readable slug from event types for Pub/Sub resource naming.
/// e.g. ["google.workspace.drive.file.v1.updated"] -> "drive-file-updated".
/// Multiple types share their common prefix: "drive-file-updated-created".
fn derive_slug_from_event_types(event_types: &[&str]) -> String {
    let parts: Vec<Vec<&str>> = event_types
        .iter()
        .map(|et| {
            let stripped = et.strip_prefix("google.workspace.").unwrap_or(et);
            stripped
                .split('.')
                .filter(|s| {
                    // Drop version segments like "v1".
                    !(s.len() <= 3
                        && s.starts_with('v')
                        && s.len() > 1
                        && s[1..].chars().all(|c| c.is_ascii_digit()))
                })
                .collect()
        })
        .collect();
    let Some(first) = parts.first() else {
        return "events".to_string();
    };
    let common_len = (0..first.len())
        .take_while(|&i| parts.iter().all(|p| p.get(i) == first.get(i)))
        .count();
    let mut segments: Vec<&str> = first[..common_len].to_vec();
    for p in &parts {
        segments.extend(p.iter().skip(common_len));
    }
    let slug: String = segments
        .join("-")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "events".to_string()
    } else {
        slug
    }
}

/// Extract the subscription name from a `subscriptions.create` long-running operation.
fn subscription_from_operation(op: &Value) -> Result<String, GwsError> {
    if let Some(err) = op.get("error") {
        return Err(GwsError::Api {
            code: err
                .get("code")
                .and_then(Value::as_u64)
                // A missing or out-of-range code is reported as 500; the
                // message below still carries the operation's own error.
                .and_then(|c| u16::try_from(c).ok())
                .unwrap_or(500),
            message: format!(
                "Workspace Events subscription creation failed: {}",
                err.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            ),
            reason: "operationFailed".to_string(),
            enable_url: None,
        });
    }
    op.get("response")
        .and_then(|r| r.get("name"))
        .and_then(Value::as_str)
        .or_else(|| op.get("metadata").and_then(|m| m.get("subscription")).and_then(Value::as_str))
        .or_else(|| op.get("name").and_then(Value::as_str).filter(|n| n.starts_with("subscriptions/")))
        .map(str::to_string)
        .ok_or_else(|| {
            GwsError::other(format!(
                "Workspace Events operation {} did not report a subscription name yet; check it with                  gwsr events operations get",
                op.get("name").and_then(Value::as_str).unwrap_or("(unnamed)")
            ))
        })
}

struct SubscribeStep<'a> {
    pubsub: PubSubClient,
    subscription: String,
    config: SubscribeConfig,
    sanitize: &'a SanitizeConfig,
    pull_timeout: Duration,
    file_counter: u64,
}

impl SubscribeStep<'_> {
    fn output(&mut self, event: &Value) -> Result<(), StepError> {
        let fatal = |e: String| StepError::Fatal(GwsError::other(e));
        match &self.config.output_dir {
            Some(dir) => {
                self.file_counter += 1;
                let ts = chrono::Utc::now().timestamp_millis();
                let path = dir.join(format!("{ts}_{}.json", self.file_counter));
                let text = serde_json::to_string_pretty(event)
                    .map_err(|e| fatal(format!("Failed to serialize event: {e}")))?;
                std::fs::write(&path, text)
                    .map_err(|e| fatal(format!("Failed to write {}: {e}", path.display())))?;
                emit_diagnostic(
                    "info",
                    "wrote",
                    json!({ "path": path.display().to_string() }),
                );
            }
            None => {
                let line = serde_json::to_string(event)
                    .map_err(|e| fatal(format!("Failed to serialize event: {e}")))?;
                let mut out = std::io::stdout().lock();
                writeln!(out, "{line}")
                    .and_then(|()| out.flush())
                    .map_err(|e| fatal(format!("Failed to write to stdout: {e}")))?;
            }
        }
        Ok(())
    }
}

impl StreamStep for SubscribeStep<'_> {
    async fn step(&mut self) -> Result<(), StepError> {
        let received = self
            .pubsub
            .pull(
                &self.subscription,
                self.config.max_messages,
                self.pull_timeout,
            )
            .await?;
        for m in &received {
            let event = decode_cloud_event(&m.message);
            // Fail closed: a block-mode sanitization error aborts before emit/ack.
            match sanitize_value(self.sanitize, event)
                .await
                .map_err(|e| StepError::Transient {
                    status: None,
                    message: e.to_string(),
                })? {
                Sanitized::Pass(v) => self.output(&v)?,
                Sanitized::Blocked(r) => emit_diagnostic(
                    "warning",
                    "blocked",
                    json!({ "ackId": m.ack_id, "filterMatchState": r.filter_match_state }),
                ),
            }
        }
        if self.config.no_ack {
            return Ok(());
        }
        let ack_ids: Vec<String> = received.into_iter().map(|m| m.ack_id).collect();
        self.pubsub.ack(&self.subscription, &ack_ids).await
    }
}

/// Resources created for this session.
struct Created {
    topic: String,
    subscription: String,
    workspace_subscription: String,
}

async fn create_resources(
    config: &SubscribeConfig,
    pubsub: &PubSubClient,
    rest: &Transport,
    events_base: &str,
) -> Result<Created, GwsError> {
    let target = config
        .target
        .as_deref()
        .ok_or_else(|| GwsError::Validation("--target is required".to_string()))?;
    let project = config
        .project
        .as_deref()
        .ok_or_else(|| GwsError::Validation("--project is required".to_string()))?;
    let types: Vec<&str> = config.event_types.iter().map(String::as_str).collect();
    let slug = derive_slug_from_event_types(&types);
    let suffix = format!("{:08x}", rand::random::<u32>());
    let topic = format!("projects/{project}/topics/gwsr-{slug}-{suffix}");
    let sub = format!("projects/{project}/subscriptions/gwsr-{slug}-{suffix}");

    emit_diagnostic("info", "creating_topic", json!({ "topic": topic }));
    pubsub.create_topic(&topic).await?;
    emit_diagnostic(
        "info",
        "creating_subscription",
        json!({ "subscription": sub }),
    );
    if let Err(e) = pubsub.create_subscription(&sub, &topic).await {
        return Err(roll_back(pubsub, &[&topic], e).await);
    }

    let body = json!({
        "targetResource": target,
        "eventTypes": config.event_types,
        "notificationEndpoint": { "pubsubTopic": topic },
        "payloadOptions": { "includeResource": true },
    });
    let op = match rest
        .json(
            reqwest::Method::POST,
            &format!("{events_base}/subscriptions"),
            &[],
            Some(&body),
            Idempotency::NonIdempotent,
            "Failed to create Workspace Events subscription",
        )
        .await
    {
        Ok(op) => op,
        Err(e) => return Err(roll_back(pubsub, &[&sub, &topic], e).await),
    };
    let workspace_subscription = match subscription_from_operation(&op) {
        Ok(name) => name,
        // A failed operation created nothing that publishes to the topic.
        // A still-running one may yet deliver there, so keep the resources.
        Err(e) if op.get("error").is_some() => {
            return Err(roll_back(pubsub, &[&sub, &topic], e).await);
        }
        Err(e) => return Err(e),
    };
    emit_diagnostic(
        "info",
        "subscribed",
        json!({ "workspaceSubscription": workspace_subscription }),
    );
    Ok(Created {
        topic,
        subscription: sub,
        workspace_subscription,
    })
}

/// Delete the Pub/Sub resources created by a setup that failed at a later
/// step: nothing will ever publish to them. A failed deletion is reported
/// alongside the original error, which is returned.
async fn roll_back(pubsub: &PubSubClient, names: &[&str], error: GwsError) -> GwsError {
    if let Err(c) = cleanup_resources(pubsub, names).await {
        emit_diagnostic(
            "error",
            "cleanup_failed",
            json!({ "message": c.to_string() }),
        );
    }
    error
}

fn dry_run_plan(matches: &ArgMatches, config: &SubscribeConfig) -> Result<(), GwsError> {
    let plan = match &config.subscription {
        Some(sub) => json!({
            "dry_run": true,
            "action": "listen to existing subscription",
            "subscription": sub,
        }),
        None => json!({
            "dry_run": true,
            "action": "create Pub/Sub topic + subscription and a Workspace Events subscription",
            "target": config.target,
            "event_types": config.event_types,
            "project": config.project,
        }),
    };
    crate::helpers::http::print_value(matches, &plan)
}

/// Handles the `+subscribe` command.
pub(super) async fn handle_subscribe(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    let config = parse_subscribe_args(matches)?;
    if crate::args::dry_run(matches)? {
        return dry_run_plan(matches, &config);
    }
    if let Some(dir) = &config.output_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| GwsError::other(format!("Failed to create {}: {e}", dir.display())))?;
    }

    let pubsub = PubSubClient::new(&Transport::for_scopes(&[PUBSUB_SCOPE]).await?);

    let (subscription, created) = match &config.subscription {
        Some(sub) => (sub.clone(), None),
        None => {
            let scopes = scopes_for_event_types(&config.event_types)?;
            let rest = Transport::for_scopes(&scopes).await?;
            let created =
                create_resources(&config, &pubsub, &rest, WORKSPACE_EVENTS_API_BASE).await?;
            (created.subscription.clone(), Some(created))
        }
    };

    let opts = StreamOptions::new(config.once, config.poll_interval, config.max_failures);
    let mut step = SubscribeStep {
        pubsub: pubsub.clone(),
        subscription: subscription.clone(),
        pull_timeout: Duration::from_secs(config.poll_interval.max(10)),
        config: config.clone(),
        sanitize: sanitize_config,
        file_counter: 0,
    };
    let result = run_stream(&opts, &mut step).await;

    let Some(created) = created else {
        return result;
    };
    if config.cleanup {
        let cleaned = cleanup_resources(&pubsub, &[&created.subscription, &created.topic]).await;
        emit_diagnostic(
            "info",
            "workspace_subscription_kept",
            json!({
                "workspaceSubscription": created.workspace_subscription,
                "note": "delete it with: gwsr events subscriptions delete",
            }),
        );
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
            "command": format!("gwsr events +subscribe --subscription {}", created.subscription),
            "subscription": created.subscription,
            "topic": created.topic,
            "workspaceSubscription": created.workspace_subscription,
        }),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::helpers::events::events_matches;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_parse_subscribe_args() {
        let m = events_matches(&[
            "+subscribe",
            "--target",
            "//chat.googleapis.com/spaces/A",
            "--event-types",
            "google.workspace.chat.message.v1.created, google.workspace.chat.message.v1.updated",
            "--project",
            "p",
            "--max-messages",
            "3",
            "--poll-interval",
            "7",
            "--no-ack",
        ]);
        let c = parse_subscribe_args(&m).unwrap();
        assert_eq!(c.event_types.len(), 2);
        assert_eq!(
            (c.max_messages, c.poll_interval, c.max_failures),
            (3, 7, 10)
        );
        assert!(c.no_ack);
    }

    #[test]
    fn test_parse_subscribe_args_validation() {
        let m = events_matches(&[
            "+subscribe",
            "--subscription",
            "projects/p/subscriptions/s",
            "--output-dir",
            "bad\x01dir",
        ]);
        assert!(parse_subscribe_args(&m).is_err());
        let m = events_matches(&[
            "+subscribe",
            "--target",
            "chat.googleapis.com/spaces/A",
            "--event-types",
            "google.workspace.chat.message.v1.created",
            "--project",
            "p",
        ]);
        assert!(
            parse_subscribe_args(&m)
                .unwrap_err()
                .to_string()
                .contains("'//'")
        );
        let m = events_matches(&[
            "+subscribe",
            "--event-types",
            "google.workspace.chat.message.v1.created",
            "--project",
            "p",
        ]);
        assert!(
            parse_subscribe_args(&m)
                .unwrap_err()
                .to_string()
                .contains("--target")
        );
        let m = events_matches(&[
            "+subscribe",
            "--target",
            "//chat.googleapis.com/spaces/A",
            "--project",
            "p",
        ]);
        assert!(
            parse_subscribe_args(&m)
                .unwrap_err()
                .to_string()
                .contains("--event-types")
        );
        let m = events_matches(&[
            "+subscribe",
            "--target",
            "//chat.googleapis.com/spaces/A",
            "--project",
            "p",
            "--event-types",
            "custom.x",
        ]);
        assert!(parse_subscribe_args(&m).is_err());
    }

    #[test]
    fn test_slugs() {
        assert_eq!(
            derive_slug_from_event_types(&["google.workspace.drive.file.v1.updated"]),
            "drive-file-updated"
        );
        assert_eq!(
            derive_slug_from_event_types(&["google.workspace.chat.message.v1.created"]),
            "chat-message-created"
        );
        assert_eq!(
            derive_slug_from_event_types(&[
                "google.workspace.drive.file.v1.updated",
                "google.workspace.drive.file.v1.created"
            ]),
            "drive-file-updated-created"
        );
        assert_eq!(
            derive_slug_from_event_types(&["custom.event.type"]),
            "custom-event-type"
        );
        assert!(
            derive_slug_from_event_types(&[
                "google.workspace.very.long.service.name.with.many.segments.v1.updated"
            ])
            .len()
                <= 40
        );
        // Non-ASCII input must not panic and must yield a valid resource name.
        let s = derive_slug_from_event_types(&["google.workspace.ünïcødé.thing.v1.x"]);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert_eq!(derive_slug_from_event_types(&[]), "events");
    }

    #[test]
    fn test_decode_cloud_event() {
        let msg = json!({
            "attributes": { "type": "google.workspace.chat.message.v1.created", "source": "//chat/spaces/A", "time": "2026-02-13T10:00:00Z" },
            "data": base64::engine::general_purpose::STANDARD.encode(r#"{"foo":"bar"}"#)
        });
        let e = decode_cloud_event(&msg);
        assert_eq!(e["type"], "google.workspace.chat.message.v1.created");
        assert_eq!(e["data"]["foo"], "bar");
        assert!(e.get("dataError").is_none());
        let bad = decode_cloud_event(&json!({ "data": "!!!" }));
        assert!(bad["data"].is_null());
        assert!(bad["dataError"].as_str().unwrap().contains("base64"));
        assert!(
            bad["type"].is_null(),
            "missing attributes are null, not 'unknown'"
        );
    }

    #[test]
    fn test_subscription_from_operation() {
        assert_eq!(
            subscription_from_operation(
                &json!({"done": true, "response": {"name": "subscriptions/abc"}})
            )
            .unwrap(),
            "subscriptions/abc"
        );
        assert_eq!(
            subscription_from_operation(
                &json!({"name": "operations/x", "metadata": {"subscription": "subscriptions/s"}})
            )
            .unwrap(),
            "subscriptions/s"
        );
        assert_eq!(
            subscription_from_operation(&json!({"name": "subscriptions/direct"})).unwrap(),
            "subscriptions/direct"
        );
        assert!(subscription_from_operation(&json!({"name": "operations/pending"})).is_err());
        let err =
            subscription_from_operation(&json!({"error": {"code": 403, "message": "denied"}}))
                .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 403, .. }));
    }

    fn config(no_ack: bool) -> SubscribeConfig {
        SubscribeConfig {
            target: None,
            event_types: vec![],
            project: None,
            subscription: Some("projects/test/subscriptions/demo".into()),
            max_messages: 10,
            poll_interval: 1,
            max_failures: 2,
            once: true,
            cleanup: false,
            no_ack,
            output_dir: None,
        }
    }

    async fn run(server: &MockServer, cfg: SubscribeConfig) -> Result<(), GwsError> {
        let sanitize = SanitizeConfig::default();
        let mut step = SubscribeStep {
            pubsub: PubSubClient::with_base(
                &Transport::for_test(&server.uri()),
                &format!("{}/v1", server.uri()),
            ),
            subscription: "projects/test/subscriptions/demo".into(),
            pull_timeout: Duration::from_secs(5),
            config: cfg,
            sanitize: &sanitize,
            file_counter: 0,
        };
        let mut opts = StreamOptions::new(true, 1, 2);
        opts.backoff_base = Duration::from_millis(1);
        opts.backoff_cap = Duration::from_millis(2);
        run_stream(&opts, &mut step).await
    }

    async fn mount_pull(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "receivedMessages": [{ "ackId": "ack-1", "message": {
                    "attributes": { "type": "t" },
                    "data": base64::engine::general_purpose::STANDARD.encode(r#"{"id":"evt-1"}"#)
                }}]
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn test_pull_emits_and_acks() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        run(&server, config(false)).await.unwrap();
    }

    #[tokio::test]
    async fn test_no_ack_skips_acknowledge() {
        let server = MockServer::start().await;
        mount_pull(&server).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:acknowledge"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        run(&server, config(true)).await.unwrap();
    }

    #[tokio::test]
    async fn test_transient_errors_are_retried_then_budget_exhausts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .respond_with(ResponseTemplate::new(503))
            .expect(3)
            .mount(&server)
            .await;
        let err = run(&server, config(false)).await.unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 503, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn test_permanent_error_stops_immediately_with_real_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/test/subscriptions/demo:pull"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(
                    json!({"error": {"code": 404, "message": "Resource not found"}}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let err = run(&server, config(false)).await.unwrap_err();
        match err {
            GwsError::Api { code, message, .. } => {
                assert_eq!(code, 404);
                assert!(message.contains("Resource not found"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    #[tokio::test]
    async fn test_failed_setup_deletes_created_pubsub_resources() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!(
                {"error": {"code": 403, "message": "caller lacks permission"}}
            )))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(2)
            .named("delete orphaned topic and subscription")
            .mount(&server)
            .await;
        let cfg = SubscribeConfig {
            target: Some("//chat.googleapis.com/spaces/A".into()),
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            project: Some("p".into()),
            subscription: None,
            cleanup: true,
            ..config(false)
        };
        let transport = Transport::for_test(&server.uri());
        let pubsub = PubSubClient::with_base(&transport, &server.uri());
        let err = create_resources(&cfg, &pubsub, &transport, &format!("{}/v1", server.uri()))
            .await
            .err()
            .expect("setup must fail");
        assert!(err.to_string().contains("caller lacks permission"), "{err}");
        let deleted: Vec<String> = server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.method == wiremock::http::Method::DELETE)
            .map(|r| r.url.path().to_string())
            .collect();
        assert_eq!(
            deleted.len(),
            2,
            "orphaned Pub/Sub resources left behind: {deleted:?}"
        );
    }
    #[tokio::test]
    async fn test_pending_operation_keeps_pubsub_resources() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"name": "operations/op1", "done": false})),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(0)
            .mount(&server)
            .await;
        let cfg = SubscribeConfig {
            target: Some("//chat.googleapis.com/spaces/A".into()),
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            project: Some("p".into()),
            subscription: None,
            ..config(false)
        };
        let transport = Transport::for_test(&server.uri());
        let pubsub = PubSubClient::with_base(&transport, &server.uri());
        let err = create_resources(&cfg, &pubsub, &transport, &format!("{}/v1", server.uri()))
            .await
            .err()
            .expect("a pending operation has no subscription name yet");
        assert!(err.to_string().contains("operations/op1"), "{err}");
    }
}
