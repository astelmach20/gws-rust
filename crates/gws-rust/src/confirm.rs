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
//!   scripts). Gated only when `GWSR_REQUIRE_CONFIRM` is `1`/`true`.
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

/// Environment variable that also gates [`Impact::Outbound`] actions.
pub(crate) const REQUIRE_CONFIRM_ENV: &str = "GWSR_REQUIRE_CONFIRM";

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

/// Whether a Discovery method is destructive.
pub(crate) fn is_destructive_method(method: &RestMethod) -> bool {
    method.http_method.eq_ignore_ascii_case("DELETE")
        || method
            .id
            .as_deref()
            .is_some_and(|id| PERMANENT_DELETE_METHODS.contains(&id))
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

/// Parse a `GWSR_REQUIRE_CONFIRM` value: unset/empty/`0`/`false` is off,
/// `1`/`true` is on, anything else is an error.
pub(crate) fn parse_policy(value: Option<&str>) -> Result<bool, GwsError> {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("" | "0" | "false") => Ok(false),
        Some("1" | "true") => Ok(true),
        Some(other) => Err(GwsError::Validation(format!(
            "{REQUIRE_CONFIRM_ENV} must be 1, true, 0 or false; got '{other}'"
        ))),
    }
}

/// Read `GWSR_REQUIRE_CONFIRM`.
pub(crate) fn policy_from_env() -> Result<bool, GwsError> {
    match std::env::var(REQUIRE_CONFIRM_ENV) {
        Ok(v) => parse_policy(Some(&v)),
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => Err(GwsError::Validation(format!(
            "{REQUIRE_CONFIRM_ENV} is not valid UTF-8"
        ))),
    }
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
        // Read the policy even when it does not matter, so a malformed value
        // always fails loudly.
        let require_outbound = policy_from_env()?;
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
    fn policy_parsing_is_strict() {
        assert!(!parse_policy(None).unwrap());
        assert!(!parse_policy(Some("")).unwrap());
        assert!(!parse_policy(Some("0")).unwrap());
        assert!(!parse_policy(Some("false")).unwrap());
        assert!(parse_policy(Some("1")).unwrap());
        assert!(parse_policy(Some("TRUE")).unwrap());
        assert!(parse_policy(Some("yes")).is_err());
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
        let bare = Command::new("t").try_get_matches_from(["t"]).unwrap();
        let err = Gate::from_matches(&bare).unwrap_err();
        assert!(err.to_string().contains("--yes"), "{err}");
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
    fn answers() {
        assert!(is_yes("y\n"));
        assert!(is_yes("YES"));
        assert!(!is_yes(""));
        assert!(!is_yes("n"));
    }
}
