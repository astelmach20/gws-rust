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

use crate::error::GwsError;
use clap::{ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;
pub mod admin;
pub mod calendar;
pub mod chat;
pub mod classroom;
pub mod docs;
pub mod drive;
pub mod events;
pub mod forms;
pub mod gmail;
pub(crate) mod http;
pub mod keep;
pub mod meet;
pub mod modelarmor;
pub mod people;
#[cfg(test)]
mod registry_tests;
pub mod script;
pub mod sheets;
pub mod slides;
pub mod tasks;
pub mod workflows;

/// Base URL for the Google Cloud Pub/Sub v1 API.
///
/// Shared across `events::subscribe` and `gmail::watch` so the constant
/// is defined in a single place.
pub(crate) const PUBSUB_API_BASE: &str = "https://pubsub.googleapis.com/v1";

/// Returns a future that completes when a shutdown signal is received.
///
/// On Unix this listens for both SIGINT (Ctrl+C) and SIGTERM; on other
/// platforms only SIGINT is handled. Used by long-running pull loops
/// (`gmail::watch`, `events::subscribe`) to exit cleanly under container
/// orchestrators (Kubernetes, Docker, systemd) that send SIGTERM.
///
/// The signal handler is registered once in a background task on first call
/// so it remains active for the lifetime of the process — no gap between
/// loop iterations.
pub(crate) async fn shutdown_signal() {
    use std::sync::OnceLock;
    use tokio::sync::Notify;

    static NOTIFY: OnceLock<std::sync::Arc<Notify>> = OnceLock::new();

    let notify = NOTIFY.get_or_init(|| {
        let n = std::sync::Arc::new(Notify::new());
        let n2 = n.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        tokio::select! {
                            res = tokio::signal::ctrl_c() => {
                                if let Err(e) = res {
                                    tracing::error!("could not listen for Ctrl+C: {e}; shutting down");
                                }
                            }
                            Some(_) = sigterm.recv() => {}
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "could not register SIGTERM handler: {e}. \
                             Listening for Ctrl+C only."
                        );
                        if let Err(e) = tokio::signal::ctrl_c().await {
                            tracing::error!("could not listen for Ctrl+C: {e}; shutting down");
                        }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                if let Err(e) = tokio::signal::ctrl_c().await {
                    tracing::error!("could not listen for Ctrl+C: {e}; shutting down");
                }
            }
            n2.notify_waiters();
        });
        n
    });

    notify.notified().await;
}

/// A trait for service-specific CLI helpers that inject custom commands.
pub trait Helper: Send + Sync {
    /// Injects subcommands into the service command.
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command;

    /// Attempts to handle a command. Returns Ok(Some(())) if handled,
    /// Ok(None) if not handled (should fall back to dynamic dispatch),
    /// or Err if handled but failed.
    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>>;

    /// If true, only helper commands are shown (discovery-generated commands are suppressed).
    fn helper_only(&self) -> bool {
        false
    }
}

/// Constructor for one helper.
type MakeHelper = fn() -> Box<dyn Helper>;

/// Helpers keyed by Discovery API name (`RestDescription::name`, i.e.
/// `ServiceEntry::api_name`, not the CLI alias).
const HELPERS: &[(&str, MakeHelper)] = &[
    ("gmail", || Box::new(gmail::GmailHelper)),
    ("sheets", || Box::new(sheets::SheetsHelper)),
    ("docs", || Box::new(docs::DocsHelper)),
    ("chat", || Box::new(chat::ChatHelper)),
    ("drive", || Box::new(drive::DriveHelper)),
    ("calendar", || Box::new(calendar::CalendarHelper)),
    ("script", || Box::new(script::ScriptHelper)),
    ("admin", || Box::new(admin::AdminHelper)),
    ("tasks", || Box::new(tasks::TasksHelper)),
    ("people", || Box::new(people::PeopleHelper)),
    ("forms", || Box::new(forms::FormsHelper)),
    ("meet", || Box::new(meet::MeetHelper)),
    ("slides", || Box::new(slides::SlidesHelper)),
    ("classroom", || Box::new(classroom::ClassroomHelper)),
    ("keep", || Box::new(keep::KeepHelper)),
    ("workspaceevents", || Box::new(events::EventsHelper)),
    ("modelarmor", || Box::new(modelarmor::ModelArmorHelper)),
    ("workflow", || Box::new(workflows::WorkflowHelper)),
];

pub fn get_helper(service: &str) -> Option<Box<dyn Helper>> {
    HELPERS
        .iter()
        .find(|(name, _)| *name == service)
        .map(|(_, make)| make())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const APP_HELPERS: &[(&str, &str)] = &[
        ("drive", "v3"),
        ("docs", "v1"),
        ("sheets", "v4"),
        ("calendar", "v3"),
        ("script", "v1"),
        ("chat", "v1"),
        ("admin", "directory_v1"),
        ("admin", "reports_v1"),
        ("tasks", "v1"),
        ("people", "v1"),
        ("forms", "v1"),
        ("meet", "v2"),
        ("slides", "v1"),
        ("classroom", "v1"),
        ("keep", "v1"),
    ];

    fn doc(name: &str, version: &str) -> crate::discovery::RestDescription {
        crate::discovery::RestDescription {
            name: name.into(),
            version: version.into(),
            root_url: "https://example.googleapis.com/".into(),
            ..Default::default()
        }
    }

    /// Every helper key is the API name of a registered service, so no entry
    /// is unreachable (as the removed `apps-script` alias was).
    #[test]
    fn every_helper_is_keyed_by_a_registered_api_name() {
        for (name, _) in HELPERS {
            assert!(
                gws_rust_core::services::SERVICES
                    .iter()
                    .any(|s| s.api_name == *name),
                "helper {name:?} is keyed by no registered API name"
            );
        }
    }

    /// Helpers must decline (not error on) Discovery-generated subcommands so
    /// the generic executor can handle them.
    #[tokio::test]
    async fn helpers_decline_non_helper_subcommands() {
        for (name, version) in APP_HELPERS {
            let helper = get_helper(name).expect("helper registered");
            let d = doc(name, version);
            let cmd = helper
                .inject_commands(
                    Command::new("gwsr").args(crate::cli_args::global_args()),
                    &d,
                )
                .subcommand(Command::new("files").subcommand(Command::new("list")));
            let m = cmd.try_get_matches_from(["gwsr", "files", "list"]).unwrap();
            let handled = helper
                .handle(&d, &m, &modelarmor::SanitizeConfig::default())
                .await
                .unwrap();
            assert!(!handled, "{name} {version}");
        }
    }

    /// Every helper command follows the flag conventions: IDs are `--<noun>-id`,
    /// there are no positional arguments, and every flag has help text.
    #[test]
    fn helper_flags_follow_conventions() {
        for (name, version) in APP_HELPERS {
            let helper = get_helper(name).expect("helper registered");
            let cmd = helper.inject_commands(Command::new("gwsr"), &doc(name, version));
            let subs: Vec<_> = cmd.get_subcommands().collect();
            assert!(!subs.is_empty(), "{name} {version} injects no helpers");
            for sub in subs {
                assert!(sub.get_name().starts_with('+'));
                let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
                assert!(
                    about.starts_with("[Helper] "),
                    "{}: {about}",
                    sub.get_name()
                );
                let first = about.trim_start_matches("[Helper] ").chars().next();
                assert!(first.is_some_and(char::is_uppercase), "{}", sub.get_name());
                for arg in sub.get_arguments() {
                    assert!(
                        arg.get_long().is_some(),
                        "{} has a positional arg",
                        sub.get_name()
                    );
                    assert!(
                        arg.get_help().is_some(),
                        "{} --{:?} lacks help",
                        sub.get_name(),
                        arg.get_long()
                    );
                    let long = arg.get_long().unwrap_or_default();
                    assert!(
                        !matches!(
                            long,
                            "document"
                                | "spreadsheet"
                                | "script"
                                | "space"
                                | "calendar"
                                | "parent"
                                | "id"
                        ),
                        "{} uses legacy flag --{long}",
                        sub.get_name()
                    );
                }
            }
        }
    }
}
