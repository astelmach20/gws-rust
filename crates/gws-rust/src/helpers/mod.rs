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
pub(crate) mod pagination;
pub mod people;
#[cfg(test)]
mod registry_tests;
pub mod script;
pub mod sheets;
pub mod slides;
pub mod tasks;
pub mod workflows;

/// Base URL `{root}{path}` for a Google API a helper calls by fixed path:
/// Google's `google_root`, or the `GWSR_API_BASE_URL` override root when one
/// is set, exactly as generated methods build their URLs.
pub(crate) fn api_base(
    endpoints: &gws_rust_core::validate::EndpointPolicy,
    google_root: &str,
    path: &str,
) -> String {
    // Both roots end with '/'.
    let root = endpoints
        .override_base()
        .map_or(google_root, |b| b.as_str());
    format!("{root}{path}")
}

/// Base URL for the Google Cloud Pub/Sub v1 API, shared across
/// `events::subscribe` and `gmail::watch`.
pub(crate) fn pubsub_api_base(endpoints: &gws_rust_core::validate::EndpointPolicy) -> String {
    api_base(endpoints, "https://pubsub.googleapis.com/", "v1")
}

/// A one-way shutdown latch: once [`Shutdown::request`] is called, every
/// current and future [`Shutdown::requested`] completes.
///
/// The request is latched rather than broadcast to whoever happens to be
/// waiting: a pull loop is not waiting between iterations (while it handles
/// a response or writes output), and a signal arriving then must still stop
/// it at its next wait instead of being lost.
struct Shutdown {
    requested: tokio::sync::Notify,
    latched: std::sync::atomic::AtomicBool,
}

impl Shutdown {
    const fn new() -> Self {
        Self {
            requested: tokio::sync::Notify::const_new(),
            latched: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn request(&self) {
        self.latched
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.requested.notify_waiters();
    }

    async fn requested(&self) {
        let notified = self.requested.notified();
        tokio::pin!(notified);
        // Register as a waiter before checking the latch, so a request made
        // between the check and the wait still wakes us.
        notified.as_mut().enable();
        if self.latched.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

/// Returns a future that completes when a shutdown signal has been received.
///
/// On Unix this listens for both SIGINT (Ctrl+C) and SIGTERM; on other
/// platforms only SIGINT is handled. Used by long-running pull loops
/// (`gmail::watch`, `events::subscribe`) to exit cleanly under container
/// orchestrators (Kubernetes, Docker, systemd) that send SIGTERM.
///
/// The signal handler is registered once in a background task on first call
/// so it remains active for the lifetime of the process, and the signal is
/// latched: one received while no loop is waiting (between iterations) still
/// completes the next call.
pub(crate) async fn shutdown_signal() {
    use std::sync::Once;

    static SHUTDOWN: Shutdown = Shutdown::new();
    static LISTEN: Once = Once::new();

    LISTEN.call_once(|| {
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
            SHUTDOWN.request();
        });
    });

    SHUTDOWN.requested().await;
}

/// A trait for service-specific CLI helpers that inject custom commands.
pub trait Helper: Send + Sync {
    /// Injects subcommands into the service command.
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command;

    /// Attempts to handle a command. Returns `Ok(true)` if handled,
    /// `Ok(false)` if not (the caller falls back to dynamic dispatch),
    /// or `Err` if handled but failed.
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

    /// A shutdown requested while nobody is waiting (a pull loop between
    /// iterations) is not lost: the next wait completes at once.
    #[tokio::test]
    async fn a_shutdown_requested_between_waits_is_not_lost() {
        let shutdown = Shutdown::new();
        shutdown.request();
        tokio::time::timeout(std::time::Duration::from_secs(5), shutdown.requested())
            .await
            .expect("a latched shutdown completes later waits");
        // A waiter registered before the request is woken too.
        let shutdown = std::sync::Arc::new(Shutdown::new());
        let waiter = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { shutdown.requested().await }
        });
        tokio::task::yield_now().await;
        shutdown.request();
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("a waiting loop is woken")
            .unwrap();
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
