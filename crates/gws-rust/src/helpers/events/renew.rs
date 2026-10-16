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

//! `events +renew`: extend (or reactivate) Workspace Events subscriptions.
//!
//! Renewing sets `ttl` to `0s`, which the API interprets as the maximum
//! allowed lifetime (`PATCH subscriptions/{id}?updateMask=ttl`).
//! `--reactivate` calls `:reactivate`, which only applies to SUSPENDED
//! subscriptions, so `--all --reactivate` selects by state, not expiry.
//! Every failure is reported; the command exits non-zero if any subscription
//! could not be renewed: `6` (retry later) when every failure is retryable,
//! `1` when any is permanent.

use super::{parse_event_types, scopes_for_event_types, workspace_events_api_base};
use crate::error::{GwsError, is_retryable_status_code};
use crate::transport::Transport;
use clap::ArgMatches;
use gws_rust_core::client::Idempotency;
use serde_json::{Value, json};

/// Read scopes for every event service the Workspace Events API supports,
/// used when `--event-types` is not given for a single subscription.
const ALL_EVENT_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/chat.messages.readonly",
    "https://www.googleapis.com/auth/chat.memberships.readonly",
    "https://www.googleapis.com/auth/chat.spaces.readonly",
    "https://www.googleapis.com/auth/meetings.space.readonly",
    "https://www.googleapis.com/auth/drive.readonly",
];

#[derive(Debug, PartialEq)]
pub(super) struct RenewConfig {
    pub subscription: Option<String>,
    pub all: bool,
    pub event_types: Vec<String>,
    pub within_secs: u64,
    pub reactivate: bool,
}

/// Normalize `SUB_ID` or `subscriptions/SUB_ID` to `subscriptions/SUB_ID`.
fn normalize_subscription(raw: &str) -> Result<String, GwsError> {
    let id = raw.strip_prefix("subscriptions/").unwrap_or(raw);
    if id.is_empty() || id.contains('/') {
        return Err(GwsError::Validation(format!(
            "Invalid --subscription-id '{raw}' (expected SUB_ID or subscriptions/SUB_ID)"
        )));
    }
    let name = format!("subscriptions/{id}");
    crate::validate::validate_resource_name(&name)?;
    Ok(name)
}

fn parse_renew_args(matches: &ArgMatches) -> Result<RenewConfig, GwsError> {
    let subscription = crate::args::value::<String>(matches, "subscription-id")?
        .map(|s| normalize_subscription(s))
        .transpose()?;
    let all = crate::args::flag(matches, "all")?;
    if subscription.is_none() && !all {
        return Err(GwsError::Validation(
            "Either --subscription-id or --all is required for +renew".to_string(),
        ));
    }
    let event_types = parse_event_types(crate::args::value::<String>(matches, "event-types")?);
    if all && event_types.is_empty() {
        return Err(GwsError::Validation(
            "--all requires --event-types".to_string(),
        ));
    }
    if !event_types.is_empty() {
        scopes_for_event_types(&event_types)?;
    }
    let within = crate::args::value::<String>(matches, "within")?
        .ok_or_else(|| GwsError::other("--within has no value"))?;
    Ok(RenewConfig {
        subscription,
        all,
        event_types,
        within_secs: parse_duration(within)?,
        reactivate: crate::args::flag(matches, "reactivate")?,
    })
}

/// Parses a duration like "90s", "30m", "1h", "2d" into seconds.
fn parse_duration(s: &str) -> Result<u64, GwsError> {
    let s = s.trim();
    let invalid = || GwsError::Validation(format!("Invalid duration '{s}' (use e.g. 30m, 1h, 2d)"));
    let unit_at = s.len().checked_sub(1).ok_or_else(invalid)?;
    if !s.is_char_boundary(unit_at) {
        return Err(invalid());
    }
    let (num_str, unit) = s.split_at(unit_at);
    let num: u64 = num_str.parse().map_err(|_| invalid())?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(invalid()),
    };
    num.checked_mul(mult).ok_or_else(invalid)
}

/// The list filter for `subscriptions.list` (required by the API).
fn list_filter(event_types: &[String]) -> String {
    event_types
        .iter()
        .map(|t| format!("event_types:\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Subscriptions from a list page that expire within `within_secs` of `now`.
fn filter_subscriptions_to_renew(
    subs: &[Value],
    now: i64,
    within_secs: u64,
) -> Result<Vec<String>, GwsError> {
    let mut result = Vec::new();
    for sub in subs {
        let name = sub.get("name").and_then(Value::as_str).ok_or_else(|| {
            GwsError::other(format!("subscriptions.list entry without name: {sub}"))
        })?;
        let Some(expire) = sub.get("expireTime").and_then(Value::as_str) else {
            continue; // no expiry: nothing to renew
        };
        let expire = chrono::DateTime::parse_from_rfc3339(expire)
            .map_err(|e| GwsError::other(format!("{name}: invalid expireTime '{expire}': {e}")))?
            .timestamp();
        let remaining = expire.saturating_sub(now);
        // Saturating: a window beyond i64 seconds covers every subscription.
        if remaining < i64::try_from(within_secs).unwrap_or(i64::MAX) {
            result.push(name.to_string());
        }
    }
    Ok(result)
}

/// Subscriptions from a list page that are SUSPENDED: the only ones
/// `:reactivate` accepts. Expiry is irrelevant for reactivation.
fn filter_subscriptions_to_reactivate(subs: &[Value]) -> Result<Vec<String>, GwsError> {
    let mut result = Vec::new();
    for sub in subs {
        let name = sub.get("name").and_then(Value::as_str).ok_or_else(|| {
            GwsError::other(format!("subscriptions.list entry without name: {sub}"))
        })?;
        if sub.get("state").and_then(Value::as_str) == Some("SUSPENDED") {
            result.push(name.to_string());
        }
    }
    Ok(result)
}

/// The error for a renewal whose long-running operation finished with an
/// `error` (a `google.rpc.Status`).
fn failed_operation(name: &str, reactivate: bool, err: &Value) -> GwsError {
    let (code, status) = crate::executor::operation::status_code(err);
    let verb = if reactivate { "reactivate" } else { "renew" };
    GwsError::Api {
        code,
        message: format!(
            "Failed to {verb} {name}: the operation failed ({status}): {}",
            err.get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ),
        reason: "operationFailed".to_string(),
        enable_url: None,
    }
}

/// Workspace Events client (base URL injectable for tests).
struct EventsApi {
    rest: Transport,
    base: String,
}

impl EventsApi {
    /// Renew (or reactivate) one subscription and return its operation. An
    /// operation that already finished with an `error` is a failure, with
    /// the class its `google.rpc.Status` code maps to, never a renewal.
    async fn renew(&self, name: &str, reactivate: bool) -> Result<Value, GwsError> {
        let op = self.send_renew(name, reactivate).await?;
        match op.get("error") {
            Some(err) => Err(failed_operation(name, reactivate, err)),
            None => Ok(op),
        }
    }

    async fn send_renew(&self, name: &str, reactivate: bool) -> Result<Value, GwsError> {
        let url = format!("{}/{name}", self.base);
        if reactivate {
            self.rest
                .json(
                    reqwest::Method::POST,
                    &format!("{url}:reactivate"),
                    &[],
                    Some(&json!({})),
                    Idempotency::Idempotent,
                    &format!("Failed to reactivate {name}"),
                )
                .await
        } else {
            self.rest
                .json(
                    reqwest::Method::PATCH,
                    &url,
                    &[("updateMask", "ttl".to_string())],
                    Some(&json!({ "ttl": "0s" })),
                    Idempotency::Idempotent,
                    &format!("Failed to renew {name}"),
                )
                .await
        }
    }

    /// List every subscription matching `filter` (all pages).
    async fn list_all(&self, filter: &str) -> Result<Vec<Value>, GwsError> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        let mut tokens = crate::helpers::pagination::PageTokens::new();
        loop {
            let mut query = vec![("filter", filter.to_string())];
            if let Some(t) = &token {
                query.push(("pageToken", t.clone()));
            }
            let page = self
                .rest
                .get_json(
                    &format!("{}/subscriptions", self.base),
                    &query,
                    "Failed to list subscriptions",
                )
                .await?;
            if let Some(subs) = page.get("subscriptions") {
                let subs = subs.as_array().ok_or_else(|| {
                    GwsError::other("subscriptions.list: 'subscriptions' is not an array")
                })?;
                out.extend(subs.iter().cloned());
            }
            match tokens.next(&page, "Failed to list subscriptions")? {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }
}

/// Renew the selected subscriptions and build the result document.
async fn run(api: &EventsApi, config: &RenewConfig, now: i64) -> Result<Value, GwsError> {
    let names = match &config.subscription {
        Some(name) => vec![name.clone()],
        None => {
            let subs = api.list_all(&list_filter(&config.event_types)).await?;
            if config.reactivate {
                filter_subscriptions_to_reactivate(&subs)?
            } else {
                filter_subscriptions_to_renew(&subs, now, config.within_secs)?
            }
        }
    };
    if let Some(name) = &config.subscription {
        // One named subscription: its failure is the command's failure, with
        // its own class (a 503 exits 6 for a retry, a 404 exits 1, a refused
        // token 2). The transport's context already names the subscription.
        let op = api.renew(name, config.reactivate).await?;
        return Ok(renew_report(
            config.reactivate,
            vec![json!({ "name": name, "operation": op })],
            Vec::new(),
        ));
    }
    let mut renewed = Vec::new();
    let mut failed = Vec::new();
    // `Some(code)` while every failure so far is retryable: the HTTP status
    // the aggregate error carries so that it is retryable too.
    let mut retry_code: Option<u16> = None;
    let mut permanent = false;
    for name in &names {
        match api.renew(name, config.reactivate).await {
            Ok(op) => renewed.push(json!({ "name": name, "operation": op })),
            Err(e) => {
                match &e {
                    // A 403 rate limit is retryable by its reason, which the
                    // aggregate does not keep: 429 says the same.
                    GwsError::Api { code, .. } if e.is_retryable() => {
                        retry_code.get_or_insert(if is_retryable_status_code(*code) {
                            *code
                        } else {
                            429
                        });
                    }
                    _ => permanent = true,
                }
                failed.push(json!({ "name": name, "error": e.to_string() }));
            }
        }
    }
    if failed.is_empty() {
        return Ok(renew_report(config.reactivate, renewed, failed));
    }
    let count = failed.len();
    let out = renew_report(config.reactivate, renewed, failed);
    // Like `gwsr batch` and `drive +sync`: a partial failure is an API
    // outcome, not an internal error. It is retryable (exit 6), as one named
    // subscription's failure would be, only when every failure is: a
    // permanent one (exit 1) is not fixed by running again.
    let code = match retry_code {
        Some(code) if !permanent => code,
        _ => 207,
    };
    Err(GwsError::Api {
        code,
        message: format!(
            "{count} of {} subscription(s) could not be renewed: {out}",
            names.len()
        ),
        reason: "renewPartialFailure".to_string(),
        enable_url: None,
    })
}

fn renew_report(reactivate: bool, renewed: Vec<Value>, failed: Vec<Value>) -> Value {
    json!({ "action": if reactivate { "reactivate" } else { "renew" }, "renewed": renewed, "failed": failed })
}

/// The `--dry-run` plan: what [`run`] would do. A named subscription is
/// renewed directly, so only `--all` lists subscriptions with a filter
/// (`--event-types` then only selects the OAuth scope).
fn dry_run_plan(config: &RenewConfig) -> Value {
    json!({
        "dry_run": true,
        "action": if config.reactivate { "reactivate" } else { "renew (ttl=0s: maximum lifetime)" },
        "subscription": config.subscription,
        "filter": config.all.then(|| list_filter(&config.event_types)),
        "withinSeconds": (config.all && !config.reactivate).then_some(config.within_secs),
        "state": (config.all && config.reactivate).then_some("SUSPENDED"),
    })
}

/// Handles the `+renew` command.
pub(super) async fn handle_renew(matches: &ArgMatches) -> Result<(), GwsError> {
    let config = parse_renew_args(matches)?;
    if crate::args::dry_run(matches)? {
        return crate::helpers::http::print_value(matches, &dry_run_plan(&config));
    }
    let scopes = if config.event_types.is_empty() {
        ALL_EVENT_SCOPES.to_vec()
    } else {
        scopes_for_event_types(&config.event_types)?
    };
    let rest = Transport::for_scopes(&scopes).await?;
    let api = EventsApi {
        base: workspace_events_api_base(rest.endpoints()),
        rest,
    };
    let out = run(&api, &config, chrono::Utc::now().timestamp()).await?;
    crate::helpers::http::print_value(matches, &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::events::events_matches;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn login_services_events_covers_all_event_scopes() {
        let events = std::iter::once("events".to_string()).collect();
        let mut granted: Vec<String> = Vec::new();
        crate::auth::scopes::append_fixed_service_scopes(&mut granted, &events, true);
        for scope in ALL_EVENT_SCOPES {
            assert!(granted.iter().any(|g| g == scope), "{scope}");
        }
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("30m").unwrap(), 1800);
        assert_eq!(parse_duration("2d").unwrap(), 172_800);
        assert_eq!(parse_duration("45s").unwrap(), 45);
        for bad in ["", "h", "1x", "abc", "1é", "99999999999999999999d"] {
            assert!(parse_duration(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn test_normalize_subscription() {
        assert_eq!(normalize_subscription("abc").unwrap(), "subscriptions/abc");
        assert_eq!(
            normalize_subscription("subscriptions/abc").unwrap(),
            "subscriptions/abc"
        );
        assert!(normalize_subscription("subscriptions/a/b").is_err());
        assert!(normalize_subscription("").is_err());
    }

    #[test]
    fn test_parse_renew_args() {
        let c = parse_renew_args(&events_matches(&["+renew", "--subscription-id", "S1"])).unwrap();
        assert_eq!(c.subscription.as_deref(), Some("subscriptions/S1"));
        assert!(!c.all && !c.reactivate);
        assert_eq!(c.within_secs, 3600);

        let c = parse_renew_args(&events_matches(&[
            "+renew",
            "--all",
            "--event-types",
            "google.workspace.chat.message.v1.created",
            "--within",
            "2d",
        ]))
        .unwrap();
        assert!(c.all);
        assert_eq!(c.within_secs, 172_800);
    }

    #[test]
    fn test_renew_flag_rules_enforced_by_clap() {
        let root = crate::commands::build_cli(&crate::discovery::RestDescription {
            name: "workspaceevents".into(),
            ..Default::default()
        });
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+renew"])
                .is_err()
        );
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+renew", "--all"])
                .is_err(),
            "--all needs --event-types"
        );
        assert!(
            root.clone()
                .try_get_matches_from([
                    "gwsr",
                    "+renew",
                    "--subscription-id",
                    "a",
                    "--all",
                    "--event-types",
                    "x"
                ])
                .is_err()
        );
        assert!(
            root.try_get_matches_from(["gwsr", "+renew", "--name", "a"])
                .is_err(),
            "--name was removed"
        );
    }

    /// The dry-run plan names a list filter only when `run` lists: with
    /// `--subscription-id`, `--event-types` just selects the OAuth scope.
    #[test]
    fn dry_run_plan_lists_a_filter_only_for_all() {
        const CHAT: &str = "google.workspace.chat.message.v1.created";
        let single = dry_run_plan(
            &parse_renew_args(&events_matches(&[
                "+renew",
                "--subscription-id",
                "S1",
                "--event-types",
                CHAT,
            ]))
            .unwrap(),
        );
        assert_eq!(single["subscription"], "subscriptions/S1");
        assert_eq!(single["filter"], Value::Null, "{single}");
        assert_eq!(single["withinSeconds"], Value::Null, "{single}");

        let all = dry_run_plan(
            &parse_renew_args(&events_matches(&["+renew", "--all", "--event-types", CHAT]))
                .unwrap(),
        );
        assert_eq!(all["subscription"], Value::Null);
        assert_eq!(all["filter"], format!("event_types:\"{CHAT}\""), "{all}");
        assert_eq!(all["withinSeconds"], 3600, "{all}");
    }

    #[test]
    fn test_list_filter() {
        assert_eq!(
            list_filter(&["a.b".into(), "c.d".into()]),
            r#"event_types:"a.b" OR event_types:"c.d""#
        );
    }

    #[test]
    fn test_filter_subscriptions_to_renew() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp();
        let subs = vec![
            json!({"name": "subscriptions/soon", "expireTime": "2026-01-01T00:30:00Z"}),
            json!({"name": "subscriptions/later", "expireTime": "2026-01-03T00:00:00Z"}),
            json!({"name": "subscriptions/expired", "expireTime": "2025-12-31T00:00:00Z"}),
            json!({"name": "subscriptions/none"}),
        ];
        assert_eq!(
            filter_subscriptions_to_renew(&subs, now, 3600).unwrap(),
            vec!["subscriptions/soon", "subscriptions/expired"]
        );
        let bad = vec![json!({"name": "subscriptions/x", "expireTime": "garbage"})];
        assert!(filter_subscriptions_to_renew(&bad, now, 3600).is_err());
    }

    fn api(server: &MockServer) -> EventsApi {
        EventsApi {
            rest: Transport::for_test(&server.uri()),
            base: format!("{}/v1", server.uri()),
        }
    }

    #[tokio::test]
    async fn test_renew_patches_ttl() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/S1"))
            .and(query_param("updateMask", "ttl"))
            .and(body_json(json!({"ttl": "0s"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "operations/1"})))
            .expect(1)
            .mount(&server)
            .await;
        let config = RenewConfig {
            subscription: Some("subscriptions/S1".into()),
            all: false,
            event_types: vec![],
            within_secs: 3600,
            reactivate: false,
        };
        let out = run(&api(&server), &config, 0).await.unwrap();
        assert_eq!(out["renewed"][0]["name"], "subscriptions/S1");
    }

    /// A renewal whose operation comes back finished with an `error` did not
    /// renew anything: it fails with the class of the operation's status
    /// code, alone or under `--all`, instead of being listed as renewed.
    #[tokio::test]
    async fn a_failed_renew_operation_is_not_a_renewal() {
        let server = MockServer::start().await;
        let failed = |code: u64, message: &str| {
            ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/x",
                "done": true,
                "error": {"code": code, "message": message}
            }))
        };
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/denied"))
            .respond_with(failed(7, "caller lacks permission"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/busy:reactivate"))
            .respond_with(failed(14, "try again"))
            .mount(&server)
            .await;
        for (id, reactivate, exit, text) in [
            (
                "denied",
                false,
                GwsError::EXIT_CODE_API,
                "caller lacks permission",
            ),
            ("busy", true, GwsError::EXIT_CODE_API_RETRYABLE, "try again"),
        ] {
            let config = RenewConfig {
                subscription: Some(format!("subscriptions/{id}")),
                all: false,
                event_types: vec![],
                within_secs: 3600,
                reactivate,
            };
            let err = run(&api(&server), &config, 0).await.unwrap_err();
            assert_eq!(err.exit_code(), exit, "{id}: {err:?}");
            let msg = err.to_string();
            assert!(msg.contains(id) && msg.contains(text), "{msg}");
        }

        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [
                    {"name": "subscriptions/ok", "expireTime": "1970-01-01T00:10:00Z"},
                    {"name": "subscriptions/denied", "expireTime": "1970-01-01T00:10:00Z"}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "operations/1"})))
            .mount(&server)
            .await;
        let config = RenewConfig {
            subscription: None,
            all: true,
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            within_secs: 3600,
            reactivate: false,
        };
        let err = run(&api(&server), &config, 0).await.unwrap_err();
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("1 of 2"), "{msg}");
        assert!(msg.contains("caller lacks permission"), "{msg}");
    }

    /// A failed renewal is an API outcome, not an internal error (exit 5):
    /// a named subscription keeps its own error class, so a cron job can
    /// retry a 503 (exit 6), and a permanent `--all` failure exits 1.
    #[tokio::test]
    async fn renew_failures_keep_the_api_error_class() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/busy"))
            .respond_with(ResponseTemplate::new(503).set_body_json(
                json!({"error": {"code": 503, "message": "unavailable", "status": "UNAVAILABLE"}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/gone"))
            .respond_with(ResponseTemplate::new(404).set_body_json(
                json!({"error": {"code": 404, "message": "not found", "status": "NOT_FOUND"}}),
            ))
            .mount(&server)
            .await;
        for (id, exit) in [
            ("busy", GwsError::EXIT_CODE_API_RETRYABLE),
            ("gone", GwsError::EXIT_CODE_API),
        ] {
            let config = RenewConfig {
                subscription: Some(format!("subscriptions/{id}")),
                all: false,
                event_types: vec![],
                within_secs: 3600,
                reactivate: false,
            };
            let err = run(&api(&server), &config, 0).await.unwrap_err();
            assert_eq!(err.exit_code(), exit, "{id}: {err:?}");
            assert!(err.to_string().contains(id), "{err}");
        }

        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [
                    {"name": "subscriptions/ok", "expireTime": "1970-01-01T00:10:00Z"},
                    {"name": "subscriptions/gone", "expireTime": "1970-01-01T00:10:00Z"}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/v1/subscriptions/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "operations/1"})))
            .mount(&server)
            .await;
        let config = RenewConfig {
            subscription: None,
            all: true,
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            within_secs: 3600,
            reactivate: false,
        };
        let err = run(&api(&server), &config, 0).await.unwrap_err();
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
        assert!(err.to_string().contains("1 of 2"), "{err}");
        assert!(err.to_string().contains("subscriptions/ok"), "{err}");
    }

    /// `--all` classifies its failures like a single `--subscription-id`
    /// would: when every failure is retryable (503, 429, a 403 rate limit)
    /// the run exits 6, so a cron job retries it; any permanent failure
    /// (here a 404) outranks them and the run exits 1, because retrying
    /// cannot fix it.
    #[tokio::test]
    async fn renew_all_exits_retryable_only_when_every_failure_is() {
        let error = |code: u16, status: &str, reason: &str| {
            ResponseTemplate::new(code).set_body_json(json!({"error": {
                "code": code, "message": status, "status": status,
                "errors": [{"reason": reason}]
            }}))
        };
        let cases: &[(&[&str], i32)] = &[
            (&["busy", "limited"], GwsError::EXIT_CODE_API_RETRYABLE),
            (&["ok", "busy", "quota"], GwsError::EXIT_CODE_API_RETRYABLE),
            (&["quota"], GwsError::EXIT_CODE_API_RETRYABLE),
            (&["busy", "gone", "limited"], GwsError::EXIT_CODE_API),
            (&["gone", "busy"], GwsError::EXIT_CODE_API),
        ];
        for (ids, exit) in cases {
            let server = MockServer::start().await;
            let subs: Vec<Value> = ids
                .iter()
                .map(|id| json!({"name": format!("subscriptions/{id}"), "expireTime": "1970-01-01T00:10:00Z"}))
                .collect();
            Mock::given(method("GET"))
                .and(path("/v1/subscriptions"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "subscriptions": subs })),
                )
                .mount(&server)
                .await;
            for (id, response) in [
                (
                    "ok",
                    ResponseTemplate::new(200).set_body_json(json!({"name": "operations/1"})),
                ),
                ("busy", error(503, "UNAVAILABLE", "backendError")),
                (
                    "limited",
                    error(429, "RESOURCE_EXHAUSTED", "rateLimitExceeded"),
                ),
                (
                    "quota",
                    error(403, "PERMISSION_DENIED", "userRateLimitExceeded"),
                ),
                ("gone", error(404, "NOT_FOUND", "notFound")),
            ] {
                Mock::given(method("PATCH"))
                    .and(path(format!("/v1/subscriptions/{id}")))
                    .respond_with(response)
                    .mount(&server)
                    .await;
            }
            let config = RenewConfig {
                subscription: None,
                all: true,
                event_types: vec!["google.workspace.chat.message.v1.created".into()],
                within_secs: 3600,
                reactivate: false,
            };
            let err = run(&api(&server), &config, 0).await.unwrap_err();
            assert_eq!(err.exit_code(), *exit, "{ids:?}: {err:?}");
            let failed = ids.iter().filter(|id| **id != "ok").count();
            assert!(
                err.to_string()
                    .contains(&format!("{failed} of {}", ids.len())),
                "{ids:?}: {err}"
            );
            assert!(
                matches!(&err, GwsError::Api { reason, .. } if reason == "renewPartialFailure"),
                "{ids:?}: {err:?}"
            );
        }
    }

    /// Listing subscriptions treats an empty nextPageToken as the last page
    /// and a repeated one as an error, never paging forever.
    #[tokio::test]
    async fn list_all_stops_on_repeated_or_empty_page_token() {
        let limit = std::time::Duration::from_secs(10);
        for (token, ok) in [("same", false), ("", true)] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/subscriptions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "subscriptions": [{"name": "subscriptions/a"}],
                    "nextPageToken": token
                })))
                .mount(&server)
                .await;
            let res = tokio::time::timeout(limit, api(&server).list_all("f"))
                .await
                .unwrap_or_else(|_| panic!("nextPageToken {token:?} must end pagination"));
            if ok {
                assert_eq!(res.unwrap().len(), 1);
            } else {
                let err = res.unwrap_err();
                assert!(err.to_string().contains("twice"), "{err}");
            }
        }
    }

    #[tokio::test]
    async fn test_all_lists_with_filter_paginates_and_reports_failures() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [{"name": "subscriptions/b", "state": "SUSPENDED", "expireTime": "1970-01-01T00:10:00Z"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .and(query_param("filter", r#"event_types:"google.workspace.chat.message.v1.created""#))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [{"name": "subscriptions/a", "state": "SUSPENDED", "expireTime": "1970-01-01T00:10:00Z"}],
                "nextPageToken": "p2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/a:reactivate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/b:reactivate"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error": {"message": "not suspended"}})),
            )
            .mount(&server)
            .await;
        let config = RenewConfig {
            subscription: None,
            all: true,
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            within_secs: 3600,
            reactivate: true,
        };
        let err = run(&api(&server), &config, 0).await.unwrap_err();
        assert!(err.to_string().contains("1 of 2"), "{err}");
    }
    #[tokio::test]
    async fn test_all_reactivate_targets_suspended_not_expiring() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [
                    // Active and about to expire: nothing to reactivate.
                    {"name": "subscriptions/active", "state": "ACTIVE",
                     "expireTime": "1970-01-01T00:10:00Z"},
                    // Suspended, expiring outside --within: must be reactivated.
                    {"name": "subscriptions/suspended", "state": "SUSPENDED",
                     "suspensionReason": "USER_SCOPE_REVOKED",
                     "expireTime": "1970-01-06T00:00:00Z"}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/suspended:reactivate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "operations/r"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/subscriptions/active:reactivate"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error": {"message": "not suspended"}})),
            )
            .expect(0)
            .mount(&server)
            .await;
        let config = RenewConfig {
            subscription: None,
            all: true,
            event_types: vec!["google.workspace.chat.message.v1.created".into()],
            within_secs: 3600,
            reactivate: true,
        };
        let out = run(&api(&server), &config, 0).await.unwrap();
        assert_eq!(
            out["renewed"],
            json!([{"name": "subscriptions/suspended", "operation": {"name": "operations/r"}}])
        );
    }
}
