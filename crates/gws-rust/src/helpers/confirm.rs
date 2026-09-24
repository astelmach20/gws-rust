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

//! Confirmation gate for destructive and outbound helper actions (SEC-25).
//!
//! Two impact classes:
//!
//! - [`Impact::Destructive`]: irreversible or high-blast-radius actions
//!   (deleting events, clearing ranges, replacing all Apps Script files,
//!   ownership transfer, public edit access). Always gated.
//! - [`Impact::Outbound`]: actions that notify or grant access to other people
//!   or run code (sending chat messages, sharing, RSVPs, inviting attendees,
//!   group membership, running scripts). Gated only when the deployment sets
//!   `GWSR_REQUIRE_CONFIRM=1` (accepted: `1`/`true`/`0`/`false`; anything else
//!   is an error).
//!
//! A gated action proceeds when `--yes` is passed. Otherwise, when stdin and
//! stderr are both terminals the user is prompted; without a terminal the
//! command fails with a message naming `--yes`. `--dry-run` never sends
//! anything and therefore bypasses the gate.

use crate::error::GwsError;
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::io::{BufRead, IsTerminal, Write};

/// Environment variable that turns on confirmation for [`Impact::Outbound`].
pub(crate) const REQUIRE_CONFIRM_ENV: &str = "GWSR_REQUIRE_CONFIRM";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Impact {
    Destructive,
    Outbound,
}

/// Add the `--yes` flag to a helper subcommand.
pub(crate) fn with_yes(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("yes")
            .long("yes")
            .short('y')
            .help("Confirm this action without prompting (required when not on a terminal)")
            .action(ArgAction::SetTrue),
    )
}

/// Build the error returned when confirmation is missing.
///
/// Single constructor so the error variant/exit code can be switched in one
/// place once core grows a dedicated confirmation variant.
pub(crate) fn confirmation_required(action: &str) -> GwsError {
    GwsError::Validation(format!(
        "Confirmation required: {action}. Re-run with --yes to proceed (or --dry-run to preview)."
    ))
}

/// Parse the `GWSR_REQUIRE_CONFIRM` policy value.
pub(crate) fn parse_policy(value: Option<&str>) -> Result<bool, GwsError> {
    match value.map(|v| v.trim().to_ascii_lowercase()) {
        None => Ok(false),
        Some(v) if v.is_empty() || v == "0" || v == "false" => Ok(false),
        Some(v) if v == "1" || v == "true" => Ok(true),
        Some(v) => Err(GwsError::Validation(format!(
            "{REQUIRE_CONFIRM_ENV} must be 1, true, 0 or false; got '{v}'"
        ))),
    }
}

fn policy_from_env() -> Result<bool, GwsError> {
    match std::env::var(REQUIRE_CONFIRM_ENV) {
        Ok(v) => parse_policy(Some(&v)),
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => Err(GwsError::Validation(format!(
            "{REQUIRE_CONFIRM_ENV} is not valid UTF-8"
        ))),
    }
}

/// Decision logic, separated from I/O for testing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Proceed,
    Prompt,
    Refuse,
}

pub(crate) fn decide(
    impact: Impact,
    policy_enabled: bool,
    yes: bool,
    dry_run: bool,
    interactive: bool,
) -> Decision {
    let gated = match impact {
        Impact::Destructive => true,
        Impact::Outbound => policy_enabled,
    };
    if !gated || yes || dry_run {
        Decision::Proceed
    } else if interactive {
        Decision::Prompt
    } else {
        Decision::Refuse
    }
}

/// Gate `action` (a short human description, e.g. "delete event X").
pub(crate) fn gate(matches: &ArgMatches, impact: Impact, action: &str) -> Result<(), GwsError> {
    let yes = super::http::flag(matches, "yes");
    let dry_run = super::http::dry_run(matches);
    // Evaluate the policy even when not needed so a malformed value always fails loudly.
    let policy = policy_from_env()?;
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    match decide(impact, policy, yes, dry_run, interactive) {
        Decision::Proceed => Ok(()),
        Decision::Refuse => Err(confirmation_required(action)),
        Decision::Prompt => prompt(action),
    }
}

fn prompt(action: &str) -> Result<(), GwsError> {
    let mut err = std::io::stderr().lock();
    write!(err, "About to {action}. Proceed? [y/N] ")
        .and_then(|()| err.flush())
        .map_err(|e| super::http::other_err(anyhow::anyhow!("Failed to write prompt: {e}")))?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| super::http::other_err(anyhow::anyhow!("Failed to read answer: {e}")))?;
    if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(GwsError::Validation(format!("Aborted: did not {action}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_always_gated() {
        assert_eq!(
            decide(Impact::Destructive, false, false, false, false),
            Decision::Refuse
        );
        assert_eq!(
            decide(Impact::Destructive, false, false, false, true),
            Decision::Prompt
        );
        assert_eq!(
            decide(Impact::Destructive, false, true, false, false),
            Decision::Proceed
        );
        assert_eq!(
            decide(Impact::Destructive, false, false, true, false),
            Decision::Proceed
        );
    }

    #[test]
    fn outbound_gated_only_by_policy() {
        assert_eq!(
            decide(Impact::Outbound, false, false, false, false),
            Decision::Proceed
        );
        assert_eq!(
            decide(Impact::Outbound, true, false, false, false),
            Decision::Refuse
        );
        assert_eq!(
            decide(Impact::Outbound, true, true, false, false),
            Decision::Proceed
        );
    }

    #[test]
    fn policy_parsing() {
        assert!(!parse_policy(None).unwrap_or(true));
        assert!(!parse_policy(Some("0")).unwrap_or(true));
        assert!(!parse_policy(Some("false")).unwrap_or(true));
        assert!(parse_policy(Some("1")).unwrap_or(false));
        assert!(parse_policy(Some("TRUE")).unwrap_or(false));
        assert!(parse_policy(Some("yes")).is_err());
    }

    #[test]
    fn error_names_yes() {
        assert!(
            confirmation_required("delete event E")
                .to_string()
                .contains("--yes")
        );
    }

    #[test]
    fn gate_refuses_without_yes_in_tests() {
        // Test harness stdin is not a terminal, so a destructive action without
        // --yes must be refused.
        let cmd = with_yes(Command::new("t")).arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue),
        );
        if std::io::stdin().is_terminal() {
            return;
        }
        let m = cmd.clone().try_get_matches_from(["t"]).ok();
        let m = m.as_ref().map(|m| gate(m, Impact::Destructive, "do it"));
        assert!(matches!(m, Some(Err(GwsError::Validation(_)))));
        let m = cmd.try_get_matches_from(["t", "--yes"]).ok();
        let m = m.as_ref().map(|m| gate(m, Impact::Destructive, "do it"));
        assert!(matches!(m, Some(Ok(()))));
    }
}
