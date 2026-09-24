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
//! subscriptions. Every failure is reported; the command exits non-zero if
//! any subscription could not be renewed.

use super::{WORKSPACE_EVENTS_API_BASE, parse_event_types, scopes_for_event_types};
use crate::error::GwsError;
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
    let subscription = matches
        .get_one::<String>("subscription-id")
        .map(|s| normalize_subscription(s))
        .transpose()?;
    let all = matches.get_flag("all");
    if subscription.is_none() && !all {
        return Err(GwsError::Validation(
            "Either --subscription-id or --all is required for +renew".to_string(),
        ));
    }
    let event_types = parse_event_types(matches.get_one::<String>("event-types"));
    if all && event_types.is_empty() {
        return Err(GwsError::Validation(
            "--all requires --event-types".to_string(),
        ));
    }
    if !event_types.is_empty() {
        scopes_for_event_types(&event_types)?;
    }
    let within = matches
        .get_one::<String>("within")
        .ok_or_else(|| GwsError::other("--within has no value"))?;
    Ok(RenewConfig {
        subscription,
        all,
        event_types,
        within_secs: parse_duration(within)?,
        reactivate: matches.get_flag("reactivate"),
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
        if remaining < i64::try_from(within_secs).unwrap_or(i64::MAX) {
            result.push(name.to_string());
        }
    }
    Ok(result)
}

/// Workspace Events client (base URL injectable for tests).
struct EventsApi {
    rest: Transport,
    base: String,
}

impl EventsApi {
    async fn renew(&self, name: &str, reactivate: bool) -> Result<Value, GwsError> {
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
            match page.get("nextPageToken").and_then(Value::as_str) {
                Some(t) => token = Some(t.to_string()),
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
            filter_subscriptions_to_renew(&subs, now, config.within_secs)?
        }
    };
    let mut renewed = Vec::new();
    let mut failed = Vec::new();
    for name in &names {
        match api.renew(name, config.reactivate).await {
            Ok(op) => renewed.push(json!({ "name": name, "operation": op })),
            Err(e) => failed.push(json!({ "name": name, "error": e.to_string() })),
        }
    }
    let out = json!({ "action": if config.reactivate { "reactivate" } else { "renew" }, "renewed": renewed, "failed": failed });
    if failed.is_empty() {
        Ok(out)
    } else {
        Err(GwsError::other(format!(
            "{} of {} subscription(s) could not be renewed: {out}",
            failed.len(),
            names.len()
        )))
    }
}

/// Handles the `+renew` command.
pub(super) async fn handle_renew(matches: &ArgMatches) -> Result<(), GwsError> {
    let config = parse_renew_args(matches)?;
    if crate::helpers::http::dry_run(matches) {
        let plan = json!({
            "dry_run": true,
            "action": if config.reactivate { "reactivate" } else { "renew (ttl=0s: maximum lifetime)" },
            "subscription": config.subscription,
            "filter": (!config.event_types.is_empty()).then(|| list_filter(&config.event_types)),
            "withinSeconds": config.all.then_some(config.within_secs),
        });
        return crate::helpers::http::print_value(matches, &plan);
    }
    let scopes = if config.event_types.is_empty() {
        ALL_EVENT_SCOPES.to_vec()
    } else {
        scopes_for_event_types(&config.event_types)?
    };
    let api = EventsApi {
        rest: Transport::for_scopes(&scopes).await?,
        base: WORKSPACE_EVENTS_API_BASE.to_string(),
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

    #[tokio::test]
    async fn test_all_lists_with_filter_paginates_and_reports_failures() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [{"name": "subscriptions/b", "expireTime": "1970-01-01T00:10:00Z"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/subscriptions"))
            .and(query_param("filter", r#"event_types:"google.workspace.chat.message.v1.created""#))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "subscriptions": [{"name": "subscriptions/a", "expireTime": "1970-01-01T00:10:00Z"}],
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
}
