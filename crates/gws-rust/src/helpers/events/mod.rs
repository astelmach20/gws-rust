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

//! Workspace Events helpers (`gwsr events +subscribe`, `+renew`) and the
//! shared Pub/Sub streaming machinery used by `gmail +watch`.

mod renew;
pub(crate) mod stream;
mod subscribe;

use super::Helper;
use crate::error::GwsError;
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;

pub struct EventsHelper;

pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";
/// Base URL for the Workspace Events v1 API (see [`crate::helpers::api_base`]).
pub(super) fn workspace_events_api_base(
    endpoints: &gws_rust_core::validate::EndpointPolicy,
) -> String {
    crate::helpers::api_base(endpoints, "https://workspaceevents.googleapis.com/", "v1")
}

/// OAuth scopes needed to manage Workspace Events subscriptions for the given
/// CloudEvent types. The Workspace Events API requires a scope that can read
/// the event data for every subscribed type.
pub(super) fn scopes_for_event_types(
    event_types: &[String],
) -> Result<Vec<&'static str>, GwsError> {
    if event_types.is_empty() {
        return Err(GwsError::Validation(
            "At least one event type is required".to_string(),
        ));
    }
    let mut scopes: Vec<&'static str> = Vec::new();
    for et in event_types {
        let service = et
            .strip_prefix("google.workspace.")
            .and_then(|rest| rest.split('.').next())
            .ok_or_else(|| {
                GwsError::Validation(format!(
                    "Unsupported event type '{et}' (expected google.workspace.<service>.<resource>.v1.<event>)"
                ))
            })?;
        let resource = et.split('.').nth(3).unwrap_or_default();
        let scope = match (service, resource) {
            ("chat", "message") => "https://www.googleapis.com/auth/chat.messages.readonly",
            ("chat", "reaction") => {
                "https://www.googleapis.com/auth/chat.messages.reactions.readonly"
            }
            ("chat", "membership") => "https://www.googleapis.com/auth/chat.memberships.readonly",
            ("chat", _) => "https://www.googleapis.com/auth/chat.spaces.readonly",
            ("meet", _) => "https://www.googleapis.com/auth/meetings.space.readonly",
            ("drive", _) => "https://www.googleapis.com/auth/drive.readonly",
            (other, _) => {
                return Err(GwsError::Validation(format!(
                    "Unsupported event service '{other}' in '{et}' (supported: chat, meet, drive)"
                )));
            }
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    Ok(scopes)
}

/// Split a comma-separated event type list.
pub(super) fn parse_event_types(raw: Option<&String>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn subscribe_cmd() -> Command {
    Command::new("+subscribe")
        .about("[Helper] Subscribe to Workspace events and stream them as NDJSON")
        .arg(
            Arg::new("target")
                .long("target")
                .help("Workspace resource URI (e.g., //chat.googleapis.com/spaces/SPACE_ID)")
                .value_name("URI"),
        )
        .arg(
            Arg::new("event-types")
                .long("event-types")
                .help("Comma-separated CloudEvents types to subscribe to")
                .value_name("TYPES"),
        )
        .arg(
            Arg::new("project")
                .long("project")
                .help("GCP project ID for Pub/Sub resources (or set GWSR_PROJECT_ID)")
                .value_name("PROJECT"),
        )
        .arg(
            Arg::new("subscription")
                .long("subscription")
                .help("Existing Pub/Sub subscription name (skip setup)")
                .value_name("NAME"),
        )
        .arg(
            Arg::new("max-messages")
                .long("max-messages")
                .help("Maximum Pub/Sub messages per pull")
                .value_parser(clap::value_parser!(u32).range(1..=1000))
                .default_value("10")
                .value_name("N"),
        )
        .arg(
            Arg::new("poll-interval")
                .long("poll-interval")
                .help("Seconds between pulls")
                .value_parser(clap::value_parser!(u64).range(1..=3600))
                .default_value("5")
                .value_name("SECS"),
        )
        .arg(
            Arg::new("max-failures")
                .long("max-failures")
                .help("Consecutive transient failures (429/5xx/network) tolerated before exiting")
                .value_parser(clap::value_parser!(u32).range(1..))
                .default_value("10")
                .value_name("N"),
        )
        .arg(
            Arg::new("once")
                .long("once")
                .help("Pull once and exit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("cleanup")
                .long("cleanup")
                .help("Delete created Pub/Sub resources on exit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-ack")
                .long("no-ack")
                .help("Do not acknowledge messages (they will be redelivered)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("output-dir")
                .long("output-dir")
                .help("Write each event to a separate JSON file in this directory")
                .value_name("DIR"),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr events +subscribe --target '//chat.googleapis.com/spaces/SPACE' --event-types 'google.workspace.chat.message.v1.created' --project my-project
  gwsr events +subscribe --subscription projects/p/subscriptions/my-sub --once
  gwsr events +subscribe --subscription projects/p/subscriptions/my-sub --cleanup --output-dir ./events

TIPS:
  stdout carries one JSON event per line. Transient errors are retried with
  backoff and reported as single-line JSON on stderr; the stream exits after
  --max-failures consecutive failures. Delivery is at-least-once.
  Without --cleanup, Pub/Sub resources persist for reconnection. Press Ctrl-C to stop.",
        )
}

fn renew_cmd() -> Command {
    Command::new("+renew")
        .about("[Helper] Renew or reactivate Workspace Events subscriptions")
        .arg(
            Arg::new("subscription-id")
                .long("subscription-id")
                .help("Subscription to renew (subscriptions/SUB_ID or SUB_ID)")
                .value_name("ID")
                .conflicts_with("all"),
        )
        .arg(
            Arg::new("all")
                .long("all")
                .help("Renew every subscription for --event-types that expires within --within")
                .action(ArgAction::SetTrue)
                .requires("event-types"),
        )
        .arg(
            Arg::new("event-types")
                .long("event-types")
                .help("Comma-separated event types (required with --all; selects the OAuth scope)")
                .value_name("TYPES"),
        )
        .arg(
            Arg::new("within")
                .long("within")
                .help("Time window for --all (e.g., 30m, 1h, 2d)")
                .default_value("1h")
                .value_name("DURATION"),
        )
        .arg(
            Arg::new("reactivate")
                .long("reactivate")
                .help("Reactivate a SUSPENDED subscription instead of extending its expiry")
                .action(ArgAction::SetTrue),
        )
        .group(
            clap::ArgGroup::new("which")
                .args(["subscription-id", "all"])
                .required(true),
        )
        .after_help(
            "\
EXAMPLES:
  gwsr events +renew --subscription-id subscriptions/SUB_ID
  gwsr events +renew --subscription-id SUB_ID --reactivate
  gwsr events +renew --all --event-types google.workspace.chat.message.v1.created --within 2d

TIPS:
  Renewing sets the subscription TTL to the maximum allowed.
  Use --all from a cron job to keep subscriptions alive.",
        )
}

impl Helper for EventsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(subscribe_cmd()).subcommand(renew_cmd())
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            match matches.subcommand() {
                Some(("+subscribe", sub)) => {
                    subscribe::handle_subscribe(sub, sanitize_config).await?
                }
                Some(("+renew", sub)) => renew::handle_renew(sub).await?,
                _ => return Ok(false),
            }
            Ok(true)
        })
    }
}

#[cfg(test)]
pub(super) fn events_matches(args: &[&str]) -> ArgMatches {
    let doc = crate::discovery::RestDescription {
        name: "workspaceevents".to_string(),
        ..Default::default()
    };
    let mut argv = vec!["gwsr"];
    argv.extend_from_slice(args);
    let m = crate::commands::build_cli(&doc)
        .try_get_matches_from(argv)
        .unwrap_or_else(|e| panic!("failed to parse {args:?}: {e}"));
    match m.subcommand() {
        Some((_, sub)) => sub.clone(),
        None => panic!("no subcommand in {args:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_commands() {
        let cmd = EventsHelper.inject_commands(
            Command::new("test"),
            &crate::discovery::RestDescription::default(),
        );
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+subscribe"));
        assert!(subcommands.contains(&"+renew"));
        cmd.debug_assert();
    }

    #[test]
    fn test_scopes_for_event_types() {
        let s = scopes_for_event_types(&[
            "google.workspace.chat.message.v1.created".into(),
            "google.workspace.chat.message.v1.updated".into(),
            "google.workspace.drive.file.v1.updated".into(),
        ])
        .unwrap();
        assert_eq!(
            s,
            vec![
                "https://www.googleapis.com/auth/chat.messages.readonly",
                "https://www.googleapis.com/auth/drive.readonly"
            ]
        );
        assert!(scopes_for_event_types(&[]).is_err());
        assert!(scopes_for_event_types(&["custom.event".into()]).is_err());
        assert!(scopes_for_event_types(&["google.workspace.keep.note.v1.created".into()]).is_err());
    }

    #[test]
    fn test_help_does_not_duplicate_defaults() {
        let cmd = EventsHelper.inject_commands(
            Command::new("t"),
            &crate::discovery::RestDescription::default(),
        );
        for sub in cmd.get_subcommands() {
            for arg in sub.get_arguments() {
                let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
                assert!(!help.contains("(default"), "{help}");
            }
        }
    }
}
