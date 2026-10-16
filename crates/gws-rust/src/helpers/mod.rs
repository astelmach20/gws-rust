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

/// The signal that asked the process to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownSignal {
    /// SIGINT, or Ctrl-C on any platform.
    Interrupt,
    /// SIGTERM (Unix only).
    Terminate,
}

impl ShutdownSignal {
    /// The shell's exit status for a process stopped by this signal: 128
    /// plus the signal number (SIGINT is 2, SIGTERM is 15). Ctrl-C on
    /// Windows counts as SIGINT.
    pub(crate) const fn exit_code(self) -> u8 {
        match self {
            ShutdownSignal::Interrupt => 130,
            ShutdownSignal::Terminate => 143,
        }
    }

    /// The signal's conventional name, for messages.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            ShutdownSignal::Interrupt => "SIGINT",
            ShutdownSignal::Terminate => "SIGTERM",
        }
    }

    const fn to_u8(self) -> u8 {
        match self {
            ShutdownSignal::Interrupt => 1,
            ShutdownSignal::Terminate => 2,
        }
    }

    const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(ShutdownSignal::Interrupt),
            2 => Some(ShutdownSignal::Terminate),
            _ => None,
        }
    }
}

/// A one-way shutdown latch: once [`Shutdown::request`] is called, every
/// current and future [`Shutdown::requested`] completes with the first
/// signal requested.
///
/// The request is latched rather than broadcast to whoever happens to be
/// waiting: a pull loop is not waiting between iterations (while it handles
/// a response or writes output), and a signal arriving then must still stop
/// it at its next wait instead of being lost.
struct Shutdown {
    requested: tokio::sync::Notify,
    /// `0` until requested, then [`ShutdownSignal::to_u8`] of the signal.
    latched: std::sync::atomic::AtomicU8,
}

impl Shutdown {
    const fn new() -> Self {
        Self {
            requested: tokio::sync::Notify::const_new(),
            latched: std::sync::atomic::AtomicU8::new(0),
        }
    }

    fn request(&self, signal: ShutdownSignal) {
        // The first signal wins; a later one only wakes the waiters again.
        if let Err(first) = self.latched.compare_exchange(
            0,
            signal.to_u8(),
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            tracing::debug!(first, "shutdown already requested");
        }
        self.requested.notify_waiters();
    }

    async fn requested(&self) -> ShutdownSignal {
        loop {
            let notified = self.requested.notified();
            tokio::pin!(notified);
            // Register as a waiter before checking the latch, so a request
            // made between the check and the wait still wakes us.
            notified.as_mut().enable();
            let latched = self.latched.load(std::sync::atomic::Ordering::SeqCst);
            if let Some(signal) = ShutdownSignal::from_u8(latched) {
                return signal;
            }
            notified.await;
        }
    }
}

/// Returns a future that completes with the signal once a shutdown signal
/// has been received.
///
/// On Unix this listens for both SIGINT (Ctrl+C) and SIGTERM; on other
/// platforms only Ctrl+C is handled. Used by long-running pull loops
/// (`gmail::watch`, `events::subscribe`) to exit cleanly under container
/// orchestrators (Kubernetes, Docker, systemd) that send SIGTERM, and by
/// `drive +sync` to save its state before exiting.
///
/// On Unix the handlers are installed synchronously on the first poll, so a
/// signal sent once the caller is waiting never meets the default
/// disposition (which would kill the process). They stay active for the
/// lifetime of the process, and the signal is latched: one received while
/// no loop is waiting (between iterations) still completes the next call.
pub(crate) async fn shutdown_signal() -> ShutdownSignal {
    use std::sync::Once;

    static SHUTDOWN: Shutdown = Shutdown::new();
    static LISTEN: Once = Once::new();

    LISTEN.call_once(|| {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let sigint = signal(SignalKind::interrupt());
            let sigterm = signal(SignalKind::terminate());
            tokio::spawn(async move {
                let received = match (sigint, sigterm) {
                    (Ok(mut sigint), Ok(mut sigterm)) => tokio::select! {
                        Some(()) = sigint.recv() => ShutdownSignal::Interrupt,
                        Some(()) = sigterm.recv() => ShutdownSignal::Terminate,
                        // Both streams ended: the runtime is shutting down.
                        else => ShutdownSignal::Interrupt,
                    },
                    (Ok(mut sigint), Err(e)) => {
                        tracing::warn!(
                            "could not register SIGTERM handler: {e}. \
                             Listening for Ctrl+C only."
                        );
                        if sigint.recv().await.is_none() {
                            tracing::debug!("SIGINT stream ended");
                        }
                        ShutdownSignal::Interrupt
                    }
                    (Err(e), _) => {
                        tracing::error!("could not listen for Ctrl+C: {e}; shutting down");
                        ShutdownSignal::Interrupt
                    }
                };
                SHUTDOWN.request(received);
            });
        }
        #[cfg(not(unix))]
        {
            tokio::spawn(async move {
                if let Err(e) = tokio::signal::ctrl_c().await {
                    tracing::error!("could not listen for Ctrl+C: {e}; shutting down");
                }
                SHUTDOWN.request(ShutdownSignal::Interrupt);
            });
        }
    });

    SHUTDOWN.requested().await
}

/// Set by a command that stopped early on a signal after finishing its
/// cleanup (`drive +sync`), so `main` exits with that signal's code.
static EXIT_SIGNAL: std::sync::OnceLock<ShutdownSignal> = std::sync::OnceLock::new();

/// Make the process exit with `signal`'s code once the command returns.
pub(crate) fn exit_for_signal(signal: ShutdownSignal) {
    EXIT_SIGNAL.get_or_init(|| signal);
}

/// The signal a command asked to exit for, if any.
pub(crate) fn exit_signal() -> Option<ShutdownSignal> {
    EXIT_SIGNAL.get().copied()
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
        shutdown.request(ShutdownSignal::Terminate);
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), shutdown.requested())
            .await
            .expect("a latched shutdown completes later waits");
        assert_eq!(got, ShutdownSignal::Terminate);
        // A waiter registered before the request is woken too.
        let shutdown = std::sync::Arc::new(Shutdown::new());
        let waiter = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { shutdown.requested().await }
        });
        tokio::task::yield_now().await;
        shutdown.request(ShutdownSignal::Interrupt);
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("a waiting loop is woken")
            .unwrap();
        assert_eq!(got, ShutdownSignal::Interrupt);
        // The first signal is the one reported.
        shutdown.request(ShutdownSignal::Terminate);
        assert_eq!(shutdown.requested().await, ShutdownSignal::Interrupt);
    }

    /// The exit codes follow the shell's 128 + signal number convention.
    #[test]
    fn shutdown_signal_exit_codes_follow_the_shell_convention() {
        assert_eq!(ShutdownSignal::Interrupt.exit_code(), 130);
        assert_eq!(ShutdownSignal::Terminate.exit_code(), 143);
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

    /// Free-text options take a value that starts with '-' (a title such as
    /// `-draft`, a Gmail query such as `-in:spam`, a body that is a Markdown
    /// list) instead of rejecting it as an unknown flag.
    #[test]
    fn free_text_options_accept_a_leading_hyphen() {
        const FREE_TEXT: &[(&str, &str, &str, &[&str])] = &[
            (
                "admin",
                "directory_v1",
                "+user-create",
                &["given-name", "family-name"],
            ),
            (
                "calendar",
                "v3",
                "+insert",
                &["summary", "location", "description"],
            ),
            (
                "calendar",
                "v3",
                "+update",
                &["summary", "location", "description"],
            ),
            ("calendar", "v3", "+agenda", &["calendar-name"]),
            ("calendar", "v3", "+rsvp", &["comment"]),
            ("chat", "v1", "+send", &["text"]),
            ("docs", "v1", "+create", &["title"]),
            ("drive", "v3", "+upload", &["name"]),
            ("drive", "v3", "+share", &["message"]),
            ("gmail", "v1", "+send", &["subject", "body"]),
            ("gmail", "v1", "+reply", &["body"]),
            ("gmail", "v1", "+reply-all", &["body"]),
            ("gmail", "v1", "+forward", &["body"]),
            ("gmail", "v1", "+triage", &["query"]),
            ("gmail", "v1", "+search", &["query"]),
            (
                "gmail",
                "v1",
                "+filter create",
                &["from", "to", "subject", "query", "exclude"],
            ),
            ("modelarmor", "v1", "+sanitize-prompt", &["text"]),
            ("modelarmor", "v1", "+sanitize-response", &["text"]),
            ("people", "v1", "+find", &["query"]),
            ("sheets", "v4", "+create", &["title", "sheet"]),
            ("slides", "v1", "+create", &["title"]),
            ("tasks", "v1", "+add", &["title", "notes"]),
            ("workflow", "v1", "+file-announce", &["message"]),
        ];
        for (name, version, sub_name, flags) in FREE_TEXT {
            let helper = get_helper(name).expect("helper registered");
            let cmd = helper.inject_commands(Command::new("gwsr"), &doc(name, version));
            let sub = sub_name.split(' ').fold(&cmd, |c, s| {
                c.find_subcommand(s)
                    .unwrap_or_else(|| panic!("{name} {sub_name} exists"))
            });
            // Only the flag under test is given, so nothing else may be
            // required.
            let groups: Vec<_> = sub.get_groups().map(|g| g.get_id().clone()).collect();
            let mut sub = sub.clone().mut_args(|a| a.required(false));
            for g in groups {
                sub = sub.mut_group(g, |g| g.required(false));
            }
            for flag in *flags {
                let m = sub
                    .clone()
                    .try_get_matches_from(["x", &format!("--{flag}"), "-draft"])
                    .unwrap_or_else(|e| panic!("{name} {sub_name} --{flag} -draft: {e}"));
                let got: Vec<&String> = m.get_many::<String>(flag).unwrap().collect();
                assert_eq!(got, ["-draft"], "{name} {sub_name} --{flag}");
            }
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
