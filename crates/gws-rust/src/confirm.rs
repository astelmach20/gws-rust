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
//!   non-DELETE methods in [`PERMANENT_DELETE_METHODS`], the device commands
//!   in [`BODY_DESTRUCTIVE_METHODS`] when their request body asks for a wipe
//!   or a deprovision, the Drive permission writes in [`PERMISSION_METHODS`]
//!   when they transfer ownership or give a whole domain or anyone edit
//!   access, the Directory user writes in [`USER_WRITE_METHODS`] when they
//!   suspend the user, and helper actions such as clearing a range or
//!   deleting a filter. Always gated.
//! - [`Impact::Outbound`]: actions that notify or grant access to other people
//!   or run code (sending mail or chat messages, sharing, RSVPs, running
//!   scripts), including the generated methods in [`OUTBOUND_METHODS`] and
//!   the Calendar event writes in [`NOTIFYING_METHODS`] when they would email
//!   attendees, the Gmail vacation responder in [`AUTO_REPLY_METHODS`] when
//!   it turns auto-replies on, and the Groups Settings writes in
//!   [`GROUP_SETTINGS_METHODS`] when they open a group to outsiders. Gated
//!   only when `GWSR_REQUIRE_CONFIRM` is `1`/`true`.
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

/// Non-DELETE API methods that permanently remove or erase data.
pub(crate) const PERMANENT_DELETE_METHODS: &[&str] = &[
    "gmail.users.messages.batchDelete",
    "calendar.calendars.clear",
    "people.people.batchDeleteContacts",
    "tasks.tasks.clear",
    "drive.files.emptyTrash",
    // Admin SDK Directory: delete Chrome printers / print servers in bulk.
    "admin.customers.chrome.printers.batchDeletePrinters",
    "admin.customers.chrome.printServers.batchDeletePrintServers",
    // Cloud Identity: erase a device, or a user's account data on a device.
    "cloudidentity.devices.wipe",
    "cloudidentity.devices.deviceUsers.wipe",
];

/// Methods that erase data, or release a device from management, only for
/// some request bodies: `(method id, body field, values that do so)`. Other
/// values of the field (approving or blocking a phone, rebooting, disabling
/// or re-enabling a Chromebook) are not gated.
pub(crate) const BODY_DESTRUCTIVE_METHODS: &[(&str, &str, &[&str])] = &[
    // Admin SDK Directory: wipe a mobile device, or the work account on it.
    (
        "directory.mobiledevices.action",
        "action",
        &["admin_remote_wipe", "admin_account_wipe"],
    ),
    // Admin SDK Directory: remove every user profile from a ChromeOS device,
    // or factory-reset it.
    (
        "admin.customer.devices.chromeos.issueCommand",
        "commandType",
        &["WIPE_USERS", "REMOTE_POWERWASH"],
    ),
    // Admin SDK Directory: deprovision ChromeOS devices. Their policies,
    // device-level printers and kiosk use are removed, and a deprovisioned
    // device must be wiped before it can be enrolled again.
    (
        "directory.chromeosdevices.action",
        "action",
        &["deprovision"],
    ),
    (
        "admin.customer.devices.chromeos.batchChangeStatus",
        "changeChromeOsDeviceStatusAction",
        &["CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_DEPROVISION"],
    ),
];

/// Whether `body` asks a [`BODY_DESTRUCTIVE_METHODS`] method `id` to wipe
/// or deprovision.
/// Values match case-insensitively, so a differently cased wipe is gated
/// rather than let through.
fn body_requests_wipe(id: &str, body: Option<&serde_json::Value>) -> bool {
    BODY_DESTRUCTIVE_METHODS
        .iter()
        .filter(|(method, _, _)| *method == id)
        .any(|(_, field, wipes)| {
            body.and_then(|b| b.get(field))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| wipes.iter().any(|w| w.eq_ignore_ascii_case(v)))
        })
}

/// Drive permission writes (`create`/`update` in v3, `insert`/`update`/
/// `patch` in v2). Like `drive +share`, they are destructive when they
/// transfer ownership or give edit-level access to a whole domain or to
/// anyone with the link, and outbound otherwise.
pub(crate) const PERMISSION_METHODS: &[&str] = &[
    "drive.permissions.create",
    "drive.permissions.insert",
    "drive.permissions.update",
    "drive.permissions.patch",
];

/// Whether a [`PERMISSION_METHODS`] call `id` transfers ownership (the
/// `transferOwnership` parameter, role `owner`, or `pendingOwner`) or gives
/// a `domain` or `anyone` grantee a role above `reader`/`commenter`. Values
/// match case-insensitively and an unexpected `transferOwnership` value
/// counts as true, so a variant is gated rather than let through.
fn transfers_ownership_or_broad_edit(
    id: &str,
    params: &serde_json::Map<String, serde_json::Value>,
    body: Option<&serde_json::Value>,
) -> bool {
    use serde_json::Value;
    if !PERMISSION_METHODS.contains(&id) {
        return false;
    }
    let transfer = match params.get("transferOwnership") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.eq_ignore_ascii_case("false"),
        Some(_) => true,
    };
    let field = |name: &str| body.and_then(|b| b.get(name));
    let role = field("role").and_then(Value::as_str);
    let owner = role.is_some_and(|r| r.eq_ignore_ascii_case("owner"))
        || field("pendingOwner").is_some_and(|v| v != &Value::Bool(false) && !v.is_null());
    let broad = field("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("domain") || t.eq_ignore_ascii_case("anyone"));
    let edit = role
        .is_some_and(|r| !r.eq_ignore_ascii_case("reader") && !r.eq_ignore_ascii_case("commenter"));
    transfer || owner || (broad && edit)
}

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
    // Emails a verification message to the send-as address.
    "gmail.users.settings.sendAs.verify",
    "gmail.users.settings.filters.create",
    "chat.spaces.messages.create",
    // Adding people to a Chat or Meet space, or creating a Chat space with
    // members, notifies them and gives them access to the space.
    "chat.spaces.members.create",
    "chat.spaces.setup",
    "meet.spaces.members.create",
    // Drive v3 and v2 sharing; see also PERMISSION_METHODS.
    "drive.permissions.create",
    "drive.permissions.insert",
    "drive.permissions.update",
    "drive.permissions.patch",
    // Calendar sharing (notifies the grantee unless sendNotifications=false,
    // and grants access either way).
    "calendar.acl.insert",
    "calendar.acl.update",
    "calendar.acl.patch",
    "script.scripts.run",
    // Deploying an Apps Script project publishes its code as a web app,
    // add-on or API executable that other people can run.
    "script.projects.deployments.create",
    "script.projects.deployments.update",
    // A Classroom guardian invitation emails the guardian, and a course
    // invitation emails the invitee.
    "classroom.userProfiles.guardianInvitations.create",
    "classroom.invitations.create",
    // Adding a student or teacher to a course notifies them and gives them
    // access to the course.
    "classroom.courses.students.create",
    "classroom.courses.teachers.create",
    // An announcement is posted to the whole class (Discovery does not say
    // whether it emails them; Classroom's notification settings decide).
    "classroom.courses.announcements.create",
    // Commenting on a Drive approval notifies its initiator and reviewers.
    "drive.approvals.comment",
    "directory.members.insert",
    // The same group membership through Cloud Identity, and admin roles.
    "cloudidentity.groups.memberships.create",
    "directory.users.makeAdmin",
    "directory.roleAssignments.insert",
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

/// Gmail's vacation responder, which auto-replies to people who write to
/// the user. Gated only when the body turns auto-replies on (see
/// [`enables_auto_reply`]).
pub(crate) const AUTO_REPLY_METHODS: &[&str] = &["gmail.users.settings.updateVacation"];

/// Whether a vacation-settings body turns auto-replies on. Any
/// `enableAutoReply` other than absent, `null` or `false` counts, so an
/// unexpected value is gated rather than let through.
fn enables_auto_reply(body: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    match body.and_then(|b| b.get("enableAutoReply")) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.eq_ignore_ascii_case("false"),
        Some(_) => true,
    }
}

/// Groups Settings writes. Gated only when the body opens the group to
/// people outside the organization (see [`GROUP_OPENING_SETTINGS`]).
pub(crate) const GROUP_SETTINGS_METHODS: &[&str] = &[
    "groupsSettings.groups.update",
    "groupsSettings.groups.patch",
];

/// `(field, value)` pairs that let any Internet user join the group or read
/// its messages, or let users outside the organization become members. The
/// API takes `allowExternalMembers` as the string `"true"`; a JSON `true`
/// counts too.
pub(crate) const GROUP_OPENING_SETTINGS: &[(&str, &str)] = &[
    ("whoCanJoin", "ANYONE_CAN_JOIN"),
    ("whoCanViewGroup", "ANYONE_CAN_VIEW"),
    ("allowExternalMembers", "true"),
];

/// Whether `body` opens a group through a [`GROUP_SETTINGS_METHODS`]
/// method `id`. Values match case-insensitively.
fn opens_group(id: &str, body: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    GROUP_SETTINGS_METHODS.contains(&id)
        && body.is_some_and(|b| {
            GROUP_OPENING_SETTINGS
                .iter()
                .any(|(field, open)| match b.get(field) {
                    Some(Value::String(s)) => s.eq_ignore_ascii_case(open),
                    Some(Value::Bool(v)) => v.to_string() == *open,
                    _ => false,
                })
        })
}

/// Directory user writes. Like `admin +user-suspend`, they are destructive
/// when the body suspends the user (see [`suspends_user`]): the user is
/// signed out and blocked at once.
pub(crate) const USER_WRITE_METHODS: &[&str] = &["directory.users.update", "directory.users.patch"];

/// Whether a [`USER_WRITE_METHODS`] body suspends the user. Any `suspended`
/// other than absent, `null` or `false` counts, so an unexpected value is
/// gated rather than let through.
fn suspends_user(id: &str, body: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    USER_WRITE_METHODS.contains(&id)
        && match body.and_then(|b| b.get("suspended")) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::String(s)) => !s.eq_ignore_ascii_case("false"),
            Some(_) => true,
        }
}

/// Whether a Discovery method is destructive.
pub(crate) fn is_destructive_method(method: &RestMethod) -> bool {
    method.http_method.eq_ignore_ascii_case("DELETE")
        || method
            .id
            .as_deref()
            .is_some_and(|id| PERMANENT_DELETE_METHODS.contains(&id))
}

/// The gate that applies to one call of a Discovery method with `params`
/// and request `body`, if any.
pub(crate) fn method_impact(
    method: &RestMethod,
    params: &serde_json::Map<String, serde_json::Value>,
    body: Option<&serde_json::Value>,
) -> Option<Impact> {
    let id = method.id.as_deref().unwrap_or_default();
    if is_destructive_method(method)
        || body_requests_wipe(id, body)
        || transfers_ownership_or_broad_edit(id, params, body)
        || suspends_user(id, body)
    {
        Some(Impact::Destructive)
    } else if OUTBOUND_METHODS.contains(&id)
        || (NOTIFYING_METHODS.contains(&id) && notifies_attendees(params))
        || (AUTO_REPLY_METHODS.contains(&id) && enables_auto_reply(body))
        || opens_group(id, body)
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
        || AUTO_REPLY_METHODS.contains(&id)
        || GROUP_SETTINGS_METHODS.contains(&id)
        || USER_WRITE_METHODS.contains(&id)
        || BODY_DESTRUCTIVE_METHODS.iter().any(|(m, _, _)| *m == id)
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
        // Bulk deletes and device wipes are POSTs, but just as irreversible.
        for id in [
            "admin.customers.chrome.printers.batchDeletePrinters",
            "admin.customers.chrome.printServers.batchDeletePrintServers",
            "cloudidentity.devices.wipe",
            "cloudidentity.devices.deviceUsers.wipe",
        ] {
            assert!(is_destructive_method(&m("POST", id)), "{id}");
            assert_eq!(
                method_impact(&m("POST", id), &serde_json::Map::new(), None),
                Some(Impact::Destructive),
                "{id}"
            );
        }
        assert!(!is_destructive_method(&m(
            "POST",
            "cloudidentity.devices.cancelWipe"
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
                method_impact(&m("POST", id), &none, None),
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
            "gmail.users.settings.sendAs.verify",
            "chat.spaces.members.create",
            "chat.spaces.setup",
            "meet.spaces.members.create",
            "cloudidentity.groups.memberships.create",
            "directory.users.makeAdmin",
            "directory.roleAssignments.insert",
        ] {
            assert_eq!(
                method_impact(&m("POST", id), &none, None),
                Some(Impact::Outbound),
                "{id}"
            );
        }
        assert_eq!(
            method_impact(&m("DELETE", "drive.permissions.delete"), &none, None),
            Some(Impact::Destructive)
        );
        assert_eq!(
            method_impact(&m("POST", "gmail.users.drafts.create"), &none, None),
            None
        );
        assert_eq!(
            method_impact(&m("GET", "drive.permissions.list"), &none, None),
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
                    method_impact(&method, &params(notify.clone()), None),
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
                    method_impact(&method, &params(quiet.clone()), None),
                    None,
                    "{id} {quiet}"
                );
            }
        }
        // Deleting an event is destructive whatever it notifies.
        assert_eq!(
            method_impact(
                &m("DELETE", "calendar.events.delete"),
                &params(serde_json::json!({"sendUpdates": "none"})),
                None
            ),
            Some(Impact::Destructive)
        );
    }

    #[test]
    fn device_commands_are_destructive_only_when_the_body_wipes() {
        use serde_json::json;
        let m = |id: &str| RestMethod {
            http_method: "POST".to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        let mobile = m("directory.mobiledevices.action");
        let chromeos = m("admin.customer.devices.chromeos.issueCommand");
        for (method, body) in [
            (&mobile, json!({"action": "admin_remote_wipe"})),
            (&mobile, json!({"action": "admin_account_wipe"})),
            (&mobile, json!({"action": "ADMIN_REMOTE_WIPE"})),
            (&chromeos, json!({"commandType": "WIPE_USERS"})),
            (&chromeos, json!({"commandType": "REMOTE_POWERWASH"})),
            (&chromeos, json!({"commandType": "remote_powerwash"})),
        ] {
            assert_eq!(
                method_impact(method, &none, Some(&body)),
                Some(Impact::Destructive),
                "{body}"
            );
        }
        for (method, body) in [
            (&mobile, Some(json!({"action": "approve"}))),
            (
                &mobile,
                Some(json!({"action": "cancel_remote_wipe_then_activate"})),
            ),
            (&mobile, Some(json!({}))),
            (&mobile, None),
            (&chromeos, Some(json!({"commandType": "REBOOT"}))),
            // The field belongs to the other method.
            (&chromeos, Some(json!({"action": "admin_remote_wipe"}))),
        ] {
            assert_eq!(
                method_impact(method, &none, body.as_ref()),
                None,
                "{body:?}"
            );
        }
        // Both take --yes, and neither is destructive by identity alone.
        for method in [&mobile, &chromeos] {
            assert!(may_be_gated(method));
            assert!(!is_destructive_method(method));
        }
        // A wipe-looking body on any other method is not gated.
        assert_eq!(
            method_impact(
                &m("directory.chromeosdevices.action"),
                &none,
                Some(&json!({"action": "admin_remote_wipe"}))
            ),
            None
        );
    }

    #[test]
    fn chromeos_deprovisions_are_destructive_only_when_the_body_asks() {
        use serde_json::json;
        let m = |id: &str| RestMethod {
            http_method: "POST".to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        let action = m("directory.chromeosdevices.action");
        let batch = m("admin.customer.devices.chromeos.batchChangeStatus");
        for (method, body) in [
            (
                &action,
                json!({"action": "deprovision", "deprovisionReason": "retiring_device"}),
            ),
            (&action, json!({"action": "DEPROVISION"})),
            (
                &batch,
                json!({"deviceIds": ["d1", "d2"],
                    "changeChromeOsDeviceStatusAction": "CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_DEPROVISION",
                    "deprovisionReason": "DEPROVISION_REASON_RETIRING_DEVICE"}),
            ),
            (
                &batch,
                json!({"changeChromeOsDeviceStatusAction": "change_chrome_os_device_status_action_deprovision"}),
            ),
        ] {
            assert_eq!(
                method_impact(method, &none, Some(&body)),
                Some(Impact::Destructive),
                "{body}"
            );
        }
        for (method, body) in [
            (&action, Some(json!({"action": "disable"}))),
            (&action, Some(json!({"action": "reenable"}))),
            (&action, None),
            (
                &batch,
                Some(
                    json!({"changeChromeOsDeviceStatusAction": "CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_DISABLE"}),
                ),
            ),
            (
                &batch,
                Some(
                    json!({"changeChromeOsDeviceStatusAction": "CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_REENABLE"}),
                ),
            ),
            // The field belongs to the other method.
            (&batch, Some(json!({"action": "deprovision"}))),
        ] {
            assert_eq!(
                method_impact(method, &none, body.as_ref()),
                None,
                "{body:?}"
            );
        }
        for method in [&action, &batch] {
            assert!(may_be_gated(method));
            assert!(!is_destructive_method(method));
        }
    }

    /// The generated Drive permission methods are gated like `drive +share`:
    /// an ownership transfer, or edit access for a whole domain or anyone
    /// with the link, is destructive (gated without `GWSR_REQUIRE_CONFIRM`).
    #[test]
    fn drive_ownership_transfers_and_broad_edit_grants_are_destructive() {
        use serde_json::json;
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let params = |v: serde_json::Value| v.as_object().cloned().unwrap_or_default();
        let user =
            |role: &str| json!({"role": role, "type": "user", "emailAddress": "a@example.com"});
        for (http, id) in [
            ("POST", "drive.permissions.create"),
            ("POST", "drive.permissions.insert"),
            ("PATCH", "drive.permissions.update"),
            ("PUT", "drive.permissions.update"),
            ("PATCH", "drive.permissions.patch"),
        ] {
            let method = m(http, id);
            assert!(may_be_gated(&method), "{id}");
            for (p, body) in [
                (json!({"transferOwnership": true}), Some(user("owner"))),
                (json!({}), Some(user("owner"))),
                (json!({}), Some(user("OWNER"))),
                (
                    json!({"transferOwnership": true}),
                    Some(json!({"role": "writer"})),
                ),
                (json!({"transferOwnership": "true"}), None),
                (
                    json!({}),
                    Some(json!({"role": "writer", "pendingOwner": true})),
                ),
                (json!({}), Some(json!({"role": "writer", "type": "anyone"}))),
                (
                    json!({}),
                    Some(
                        json!({"role": "fileOrganizer", "type": "domain", "domain": "example.com"}),
                    ),
                ),
                (
                    json!({}),
                    Some(json!({"role": "organizer", "type": "Domain", "domain": "example.com"})),
                ),
            ] {
                assert_eq!(
                    method_impact(&method, &params(p.clone()), body.as_ref()),
                    Some(Impact::Destructive),
                    "{id} {p} {body:?}"
                );
            }
            for (p, body) in [
                (json!({}), Some(user("writer"))),
                (json!({"transferOwnership": false}), Some(user("reader"))),
                (json!({}), Some(json!({"role": "reader", "type": "anyone"}))),
                (
                    json!({}),
                    Some(json!({"role": "commenter", "type": "domain", "domain": "example.com"})),
                ),
                (
                    json!({}),
                    Some(json!({"role": "writer", "pendingOwner": false})),
                ),
                (json!({}), None),
            ] {
                assert_eq!(
                    method_impact(&method, &params(p.clone()), body.as_ref()),
                    Some(Impact::Outbound),
                    "{id} {p} {body:?}"
                );
            }
        }
        // The same body on another method is not a Drive grant.
        assert_eq!(
            method_impact(
                &m("POST", "drive.files.create"),
                &params(json!({})),
                Some(&json!({"role": "owner", "type": "anyone"}))
            ),
            None
        );
    }

    /// Deploying Apps Script code, emailing a Classroom guardian and
    /// commenting on a Drive approval are outbound whatever the body.
    #[test]
    fn deployments_guardian_invitations_and_approval_comments_are_outbound() {
        let none = serde_json::Map::new();
        for (http, id) in [
            ("POST", "script.projects.deployments.create"),
            ("PUT", "script.projects.deployments.update"),
            ("POST", "classroom.userProfiles.guardianInvitations.create"),
            ("POST", "drive.approvals.comment"),
        ] {
            let method = RestMethod {
                http_method: http.to_string(),
                id: Some(id.to_string()),
                ..Default::default()
            };
            assert!(may_be_gated(&method), "{id}");
            assert_eq!(
                method_impact(&method, &none, None),
                Some(Impact::Outbound),
                "{id}"
            );
        }
        // Reading deployments, or creating a version, is not gated.
        for (http, id) in [
            ("GET", "script.projects.deployments.list"),
            ("POST", "script.projects.versions.create"),
            ("POST", "drive.approvals.approve"),
        ] {
            let method = RestMethod {
                http_method: http.to_string(),
                id: Some(id.to_string()),
                ..Default::default()
            };
            assert!(!may_be_gated(&method), "{id}");
            assert_eq!(method_impact(&method, &none, None), None, "{id}");
        }
    }

    /// The vacation responder emails whoever writes to the user, so turning
    /// it on is outbound; turning it off is not.
    #[test]
    fn vacation_responder_is_outbound_only_when_enabled() {
        use serde_json::json;
        let method = RestMethod {
            http_method: "PUT".to_string(),
            id: Some("gmail.users.settings.updateVacation".to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        assert!(may_be_gated(&method));
        for body in [
            json!({"enableAutoReply": true, "responseSubject": "Away"}),
            json!({"enableAutoReply": "true"}),
            json!({"enableAutoReply": "yes"}),
            json!({"enableAutoReply": 1}),
        ] {
            assert_eq!(
                method_impact(&method, &none, Some(&body)),
                Some(Impact::Outbound),
                "{body}"
            );
        }
        for body in [
            Some(json!({"enableAutoReply": false})),
            Some(json!({"enableAutoReply": "FALSE"})),
            Some(json!({"enableAutoReply": null})),
            Some(json!({"responseSubject": "Away"})),
            None,
        ] {
            assert_eq!(
                method_impact(&method, &none, body.as_ref()),
                None,
                "{body:?}"
            );
        }
        // The field on another Gmail settings method is not gated.
        let pop = RestMethod {
            http_method: "PUT".to_string(),
            id: Some("gmail.users.settings.updatePop".to_string()),
            ..Default::default()
        };
        assert_eq!(
            method_impact(&pop, &none, Some(&json!({"enableAutoReply": true}))),
            None
        );
    }

    /// Opening a group to anyone on the Internet, or to external members,
    /// grants access to people outside the organization.
    #[test]
    fn group_settings_are_outbound_only_when_they_open_the_group() {
        use serde_json::json;
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        for method in [
            m("PUT", "groupsSettings.groups.update"),
            m("PATCH", "groupsSettings.groups.patch"),
        ] {
            assert!(may_be_gated(&method));
            for body in [
                json!({"whoCanJoin": "ANYONE_CAN_JOIN"}),
                json!({"whoCanJoin": "anyone_can_join"}),
                json!({"whoCanViewGroup": "ANYONE_CAN_VIEW", "name": "g"}),
                json!({"allowExternalMembers": "true"}),
                json!({"allowExternalMembers": "TRUE"}),
                json!({"allowExternalMembers": true}),
                json!({"whoCanJoin": "INVITED_CAN_JOIN", "allowExternalMembers": "true"}),
            ] {
                assert_eq!(
                    method_impact(&method, &none, Some(&body)),
                    Some(Impact::Outbound),
                    "{body}"
                );
            }
            for body in [
                Some(json!({"whoCanJoin": "ALL_IN_DOMAIN_CAN_JOIN"})),
                Some(json!({"whoCanViewGroup": "ALL_MEMBERS_CAN_VIEW"})),
                Some(json!({"allowExternalMembers": "false"})),
                Some(json!({"allowExternalMembers": false})),
                Some(json!({"description": "ANYONE_CAN_JOIN"})),
                Some(json!({})),
                None,
            ] {
                assert_eq!(
                    method_impact(&method, &none, body.as_ref()),
                    None,
                    "{body:?}"
                );
            }
        }
        // Reading a group's settings is not gated.
        let get = m("GET", "groupsSettings.groups.get");
        assert!(!may_be_gated(&get));
        assert_eq!(
            method_impact(&get, &none, Some(&json!({"whoCanJoin": "ANYONE_CAN_JOIN"}))),
            None
        );
    }

    /// Inviting someone to a course, adding a student or teacher, and
    /// posting an announcement reach other people whatever the body.
    #[test]
    fn classroom_invitations_enrollments_and_announcements_are_outbound() {
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        for id in [
            "classroom.invitations.create",
            "classroom.courses.students.create",
            "classroom.courses.teachers.create",
            "classroom.courses.announcements.create",
        ] {
            let method = m("POST", id);
            assert!(may_be_gated(&method), "{id}");
            assert!(!is_destructive_method(&method), "{id}");
            assert_eq!(
                method_impact(&method, &none, None),
                Some(Impact::Outbound),
                "{id}"
            );
        }
        // Reading rosters, accepting an invitation, creating a course or
        // editing an existing announcement is not gated.
        for (http, id) in [
            ("GET", "classroom.courses.students.list"),
            ("GET", "classroom.invitations.list"),
            ("POST", "classroom.invitations.accept"),
            ("POST", "classroom.courses.create"),
            ("PATCH", "classroom.courses.announcements.patch"),
        ] {
            let method = m(http, id);
            assert!(!may_be_gated(&method), "{id}");
            assert_eq!(method_impact(&method, &none, None), None, "{id}");
        }
    }

    /// Suspending a user through the generated Directory method is
    /// destructive, like `admin +user-suspend`; other user edits, and
    /// unsuspending, are not gated.
    #[test]
    fn user_writes_are_destructive_only_when_they_suspend() {
        use serde_json::json;
        let m = |http: &str, id: &str| RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        };
        let none = serde_json::Map::new();
        for method in [
            m("PUT", "directory.users.update"),
            m("PATCH", "directory.users.patch"),
        ] {
            assert!(may_be_gated(&method));
            assert!(!is_destructive_method(&method));
            for body in [
                json!({"suspended": true}),
                json!({"suspended": true, "orgUnitPath": "/Leavers"}),
                json!({"suspended": "true"}),
                json!({"suspended": "TRUE"}),
                json!({"suspended": 1}),
            ] {
                assert_eq!(
                    method_impact(&method, &none, Some(&body)),
                    Some(Impact::Destructive),
                    "{body}"
                );
            }
            for body in [
                Some(json!({"suspended": false})),
                Some(json!({"suspended": "false"})),
                Some(json!({"suspended": null})),
                Some(json!({"name": {"givenName": "Ann"}})),
                Some(json!({"suspensionReason": "ADMIN"})),
                Some(json!({})),
                None,
            ] {
                assert_eq!(
                    method_impact(&method, &none, body.as_ref()),
                    None,
                    "{body:?}"
                );
            }
        }
        // The field on another Directory method is not a suspension.
        let insert = m("POST", "directory.users.insert");
        assert!(!may_be_gated(&insert));
        assert_eq!(
            method_impact(&insert, &none, Some(&json!({"suspended": true}))),
            None
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
