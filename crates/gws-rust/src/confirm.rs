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

//! The one confirmation gate (SEC-25), shared by generated API methods,
//! `gwsr batch`, and every helper.
//!
//! Two impact classes:
//!
//! - [`Impact::Destructive`]: irreversible actions — any `DELETE` method, the
//!   non-DELETE methods in [`PERMANENT_DELETE_METHODS`], and helper actions
//!   such as clearing a range or deleting a filter. Always gated.
//! - [`Impact::Outbound`]: actions that notify or grant access to other people
//!   or run code (sending mail or chat messages, sharing, RSVPs, running
//!   scripts), including the generated methods in [`OUTBOUND_METHODS`] and
//!   the Calendar event writes in [`NOTIFYING_METHODS`] when they would email
//!   attendees. Gated only when `GWSR_REQUIRE_CONFIRM` is `1`/`true`.
//!
//! | gated action           | `--yes` or `--dry-run` | terminal | no terminal |
//! |------------------------|------------------------|----------|-------------|
//! |                        | proceed, no prompt     | prompt   | refuse      |
//!
//! A refusal or a declined prompt is [`GwsError::ConfirmationRequired`]
//! (exit code 7); nothing is sent. `--dry-run` never prompts.

use std::io::{BufRead, IsTerminal, Write};

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::discovery::RestMethod;
use crate::error::GwsError;

/// Non-DELETE API methods that permanently remove data.
pub(crate) const PERMANENT_DELETE_METHODS: &[&str] = &[
    "gmail.users.messages.batchDelete",
    "calendar.calendars.clear",
    "people.people.batchDeleteContacts",
    "tasks.tasks.clear",
    "drive.files.emptyTrash",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Impact {
    /// Irreversible changes. Always gated.
    Destructive,
    /// Visible to other people. Gated when `GWSR_REQUIRE_CONFIRM=1`.
    Outbound,
}

/// API methods that send mail or messages, share files or calendars, route
/// mail to other people, or run code: the generated-method counterparts of
/// the outbound helpers (`gmail +send`, `gmail +filter create --forward`,
/// `chat +send`, `drive +share`, `script +run`, `admin +group-add-member`).
pub(crate) const OUTBOUND_METHODS: &[&str] = &[
    "gmail.users.messages.send",
    "gmail.users.drafts.send",
    // Mailbox access for someone else, and mail routed to other addresses.
    "gmail.users.settings.delegates.create",
    "gmail.users.settings.updateAutoForwarding",
    "gmail.users.settings.forwardingAddresses.create",
    "gmail.users.settings.sendAs.create",
    "gmail.users.settings.filters.create",
    "chat.spaces.messages.create",
    "drive.permissions.create",
    "drive.permissions.update",
    // Calendar sharing (notifies the grantee unless sendNotifications=false,
    // and grants access either way).
    "calendar.acl.insert",
    "calendar.acl.update",
    "calendar.acl.patch",
    "script.scripts.run",
    "directory.members.insert",
];

/// Calendar event writes that email attendees only when asked to:
/// `sendUpdates` is `all` or `externalOnly`, or the deprecated
/// `sendNotifications` is `true`. Discovery gives neither parameter a
/// default and documents the absent case as "no notifications", so a call
/// without them is not gated. (`calendar.events.delete` also takes these
/// parameters, but as a `DELETE` it is always gated as destructive.)
pub(crate) const NOTIFYING_METHODS: &[&str] = &[
    "calendar.events.insert",
    "calendar.events.update",
    "calendar.events.patch",
    "calendar.events.move",
    "calendar.events.quickAdd",
];

/// Whether request parameters ask Calendar to email attendees. Any
/// `sendUpdates` value other than `none` counts, so an unexpected value is
/// gated rather than let through.
fn notifies_attendees(params: &serde_json::Map<String, serde_json::Value>) -> bool {
    use serde_json::Value;
    let send_updates = match params.get("sendUpdates") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => s != "none",
        Some(_) => true,
    };
    let send_notifications = match params.get("sendNotifications") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.eq_ignore_ascii_case("false"),
        Some(_) => true,
    };
    send_updates || send_notifications
}

/// Whether a Discovery method is destructive.
pub(crate) fn is_destructive_method(method: &RestMethod) -> bool {
    method.http_method.eq_ignore_ascii_case("DELETE")
        || method
            .id
            .as_deref()
            .is_some_and(|id| PERMANENT_DELETE_METHODS.contains(&id))
}

/// The gate that applies to one call of a Discovery method with `params`,
/// if any.
pub(crate) fn method_impact(
    method: &RestMethod,
    params: &serde_json::Map<String, serde_json::Value>,
) -> Option<Impact> {
    let id = method.id.as_deref().unwrap_or_default();
    if is_destructive_method(method) {
        Some(Impact::Destructive)
    } else if OUTBOUND_METHODS.contains(&id)
        || (NOTIFYING_METHODS.contains(&id) && notifies_attendees(params))
    {
        Some(Impact::Outbound)
    } else {
        None
    }
}

/// Whether some call of `method` can be gated, so its command takes `--yes`.
pub(crate) fn may_be_gated(method: &RestMethod) -> bool {
    let id = method.id.as_deref().unwrap_or_default();
    is_destructive_method(method)
        || OUTBOUND_METHODS.contains(&id)
        || NOTIFYING_METHODS.contains(&id)
}

/// The `--yes`/`-y` flag, identical on every gated command.
pub(crate) fn yes_arg() -> Arg {
    Arg::new("yes")
        .long("yes")
        .short('y')
        .help("Confirm this action without prompting (required when not on a terminal)")
        .action(ArgAction::SetTrue)
}

/// Add [`yes_arg`] to a helper subcommand.
pub(crate) fn with_yes(cmd: Command) -> Command {
    cmd.arg(yes_arg())
}

/// Whether both stdin and stderr are terminals (a human can answer).
pub(crate) fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Proceed,
    Prompt,
    Refuse,
}

/// The gate's decision table, free of I/O.
pub(crate) fn decide(
    impact: Impact,
    require_outbound: bool,
    yes: bool,
    dry_run: bool,
    interactive: bool,
) -> Decision {
    let gated = match impact {
        Impact::Destructive => true,
        Impact::Outbound => require_outbound,
    };
    if !gated || yes || dry_run {
        Decision::Proceed
    } else if interactive {
        Decision::Prompt
    } else {
        Decision::Refuse
    }
}

/// Everything the gate needs to know about one invocation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gate {
    pub yes: bool,
    pub dry_run: bool,
    pub interactive: bool,
}

impl Gate {
    /// `--yes`/`--dry-run` from `matches`, and whether a terminal is attached.
    pub(crate) fn from_matches(matches: &ArgMatches) -> Result<Self, GwsError> {
        Ok(Self {
            yes: crate::args::flag(matches, "yes")?,
            dry_run: crate::args::dry_run(matches)?,
            interactive: is_interactive(),
        })
    }

    /// Decide whether `action` (e.g. "delete filter f1") may proceed,
    /// prompting through `ask` when needed.
    pub(crate) fn check_with(
        self,
        impact: Impact,
        action: &str,
        ask: impl FnOnce(&str) -> Result<bool, GwsError>,
    ) -> Result<(), GwsError> {
        let require_outbound = crate::env::get()?.require_confirm;
        match decide(
            impact,
            require_outbound,
            self.yes,
            self.dry_run,
            self.interactive,
        ) {
            Decision::Proceed => Ok(()),
            Decision::Refuse => Err(GwsError::ConfirmationRequired(format!(
                "Confirmation required: {action}. Re-run with --yes to proceed (or --dry-run to preview)."
            ))),
            Decision::Prompt => {
                if ask(&format!("About to {action}. Proceed? [y/N] "))? {
                    Ok(())
                } else {
                    Err(GwsError::ConfirmationRequired(format!(
                        "Aborted: did not {action} (declined at the prompt)"
                    )))
                }
            }
        }
    }

    /// [`Gate::check_with`] prompting on the terminal.
    pub(crate) fn check(self, impact: Impact, action: &str) -> Result<(), GwsError> {
        self.check_with(impact, action, ask_on_terminal)
    }
}

/// Gate a helper action using its `--yes`/`--dry-run` flags.
pub(crate) fn confirm(matches: &ArgMatches, impact: Impact, action: &str) -> Result<(), GwsError> {
    Gate::from_matches(matches)?.check(impact, action)
}

/// Prompt on stderr and read a y/N answer from stdin.
fn ask_on_terminal(prompt: &str) -> Result<bool, GwsError> {
    let mut err = std::io::stderr().lock();
    write!(err, "{prompt}")
        .and_then(|()| err.flush())
        .map_err(|e| {
            GwsError::other(anyhow::anyhow!("failed to write confirmation prompt: {e}"))
        })?;
    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| GwsError::other(anyhow::anyhow!("failed to read confirmation: {e}")))?;
    Ok(is_yes(&answer))
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never_ask(_: &str) -> Result<bool, GwsError> {
        panic!("must not prompt")
    }

    fn gate(yes: bool, dry_run: bool, interactive: bool) -> Gate {
        Gate {
            yes,
            dry_run,
            interactive,
        }
    }

    #[test]
    fn destructive_is_always_gated() {
        use Decision::*;
        assert_eq!(
            decide(Impact::Destructive, false, false, false, false),
            Refuse
        );
        assert_eq!(
            decide(Impact::Destructive, false, false, false, true),
            Prompt
        );
        assert_eq!(
            decide(Impact::Destructive, false, true, false, false),
            Proceed
        );
        assert_eq!(
            decide(Impact::Destructive, false, false, true, false),
            Proceed
        );
    }

    #[test]
    fn outbound_is_gated_only_by_policy() {
        use Decision::*;
        assert_eq!(
            decide(Impact::Outbound, false, false, false, false),
            Proceed
        );
        assert_eq!(decide(Impact::Outbound, true, false, false, false), Refuse);
        assert_eq!(decide(Impact::Outbound, true, false, false, true), Prompt);
        assert_eq!(decide(Impact::Outbound, true, true, false, false), Proceed);
    }

    #[test]
    fn refusal_and_decline_are_confirmation_required() {
        let err = gate(false, false, false)
            .check_with(Impact::Destructive, "delete filter f1", never_ask)
            .unwrap_err();
        assert!(matches!(err, GwsError::ConfirmationRequired(_)), "{err:?}");
        assert!(err.to_string().contains("--yes"));
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_CONFIRMATION_REQUIRED);

        let err = gate(false, false, true)
            .check_with(Impact::Destructive, "x", |_| Ok(false))
            .unwrap_err();
        assert!(matches!(err, GwsError::ConfirmationRequired(ref m) if m.contains("Aborted")));
        gate(false, false, true)
            .check_with(Impact::Destructive, "x", |p| {
                assert!(p.contains("About to x"));
                Ok(true)
            })
            .unwrap();
    }

    #[test]
    fn yes_and_dry_run_never_prompt() {
        gate(true, false, true)
            .check_with(Impact::Destructive, "x", never_ask)
            .unwrap();
        gate(false, true, true)
            .check_with(Impact::Destructive, "x", never_ask)
            .unwrap();
    }

    #[test]
    fn gate_reads_flags_and_rejects_commands_without_them() {
        let cmd = with_yes(Command::new("t")).arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue),
        );
        let g =
            Gate::from_matches(&cmd.clone().try_get_matches_from(["t", "-y"]).unwrap()).unwrap();
        assert!(g.yes && !g.dry_run);
        let g = Gate::from_matches(&cmd.try_get_matches_from(["t", "--dry-run"]).unwrap()).unwrap();
        assert!(g.dry_run);
        // A gated command that does not define --yes is a programming error,
        // reported instead of silently refusing every non-interactive run.
        // clap detects undefined ids only with debug assertions (see crate::args).
        #[cfg(debug_assertions)]
        {
            let bare = Command::new("t").try_get_matches_from(["t"]).unwrap();
            let err = Gate::from_matches(&bare).unwrap_err();
            assert!(err.to_string().contains("--yes"), "{err}");
        }
    }

    #[test]
    fn destructive_method_detection() {
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        assert!(is_destructive_method(&m("DELETE", "drive.files.delete")));
        assert!(is_destructive_method(&m(
            "POST",
            "gmail.users.messages.batchDelete"
        )));
        assert!(!is_destructive_method(&m("GET", "drive.files.list")));
        assert!(!is_destructive_method(&m(
            "POST",
            "gmail.users.messages.trash"
        )));
    }

    #[test]
    fn method_impact_classifies_outbound_methods() {
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        for id in OUTBOUND_METHODS {
            assert_eq!(
                method_impact(&m("POST", id), &none),
                Some(Impact::Outbound),
                "{id}"
            );
            assert!(may_be_gated(&m("POST", id)), "{id}");
        }
        for id in [
            "calendar.acl.insert",
            "calendar.acl.patch",
            "gmail.users.settings.delegates.create",
            "gmail.users.settings.updateAutoForwarding",
            "gmail.users.settings.forwardingAddresses.create",
            "gmail.users.settings.sendAs.create",
            "gmail.users.settings.filters.create",
        ] {
            assert_eq!(
                method_impact(&m("POST", id), &none),
                Some(Impact::Outbound),
                "{id}"
            );
        }
        assert_eq!(
            method_impact(&m("DELETE", "drive.permissions.delete"), &none),
            Some(Impact::Destructive)
        );
        assert_eq!(
            method_impact(&m("POST", "gmail.users.drafts.create"), &none),
            None
        );
        assert_eq!(
            method_impact(&m("GET", "drive.permissions.list"), &none),
            None
        );
        assert!(!may_be_gated(&m("GET", "drive.permissions.list")));
    }

    #[test]
    fn calendar_event_writes_are_outbound_only_when_notifying() {
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let params = |v: serde_json::Value| v.as_object().cloned().unwrap_or_default();
        for id in NOTIFYING_METHODS {
            let method = m("POST", id);
            assert!(may_be_gated(&method), "{id}");
            for notify in [
                serde_json::json!({"sendUpdates": "all"}),
                serde_json::json!({"sendUpdates": "externalOnly"}),
                serde_json::json!({"sendUpdates": "surprise"}),
                serde_json::json!({"sendNotifications": true}),
                serde_json::json!({"sendNotifications": "true"}),
                serde_json::json!({"sendUpdates": "none", "sendNotifications": true}),
            ] {
                assert_eq!(
                    method_impact(&method, &params(notify.clone())),
                    Some(Impact::Outbound),
                    "{id} {notify}"
                );
            }
            for quiet in [
                serde_json::json!({}),
                serde_json::json!({"sendUpdates": "none"}),
                serde_json::json!({"sendNotifications": false}),
                serde_json::json!({"sendNotifications": "false"}),
            ] {
                assert_eq!(
                    method_impact(&method, &params(quiet.clone())),
                    None,
                    "{id} {quiet}"
                );
            }
        }
        // Deleting an event is destructive whatever it notifies.
        assert_eq!(
            method_impact(
                &m("DELETE", "calendar.events.delete"),
                &params(serde_json::json!({"sendUpdates": "none"}))
            ),
            Some(Impact::Destructive)
        );
    }

    #[test]
    fn answers() {
        assert!(is_yes("y\n"));
        assert!(is_yes("YES"));
        assert!(!is_yes(""));
        assert!(!is_yes("n"));
    }
}
