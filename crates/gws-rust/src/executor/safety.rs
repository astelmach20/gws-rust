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

//! Confirmation gate for destructive methods.
//!
//! A method is *destructive* when its HTTP method is `DELETE` or its id is in
//! [`PERMANENT_DELETE_METHODS`] (non-DELETE methods that irreversibly remove
//! data). Behaviour is controlled by `GWSR_CONFIRM_DESTRUCTIVE`:
//!
//! | value            | interactive (stdin and stderr are TTYs) | non-interactive   |
//! |------------------|-----------------------------------------|-------------------|
//! | unset (default)  | prompt unless `--yes`                   | run               |
//! | `1` / `true`     | prompt unless `--yes`                   | require `--yes`   |
//! | `0` / `false`    | run                                     | run               |
//!
//! `--dry-run` never prompts because nothing is sent.

use std::io::{BufRead, IsTerminal, Write};

use crate::discovery::RestMethod;
use crate::error::GwsError;

pub(crate) const CONFIRM_ENV: &str = "GWSR_CONFIRM_DESTRUCTIVE";

/// Non-DELETE methods that permanently remove data.
pub(crate) const PERMANENT_DELETE_METHODS: &[&str] = &[
    "gmail.users.messages.batchDelete",
    "calendar.calendars.clear",
    "people.people.batchDeleteContacts",
    "tasks.tasks.clear",
    "drive.files.emptyTrash",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmPolicy {
    /// Never ask.
    Off,
    /// Ask on a terminal; run unattended scripts without asking (default).
    #[default]
    Interactive,
    /// Ask on a terminal; refuse unattended runs without `--yes`.
    Always,
}

impl ConfirmPolicy {
    pub(crate) fn from_env() -> Result<Self, GwsError> {
        match std::env::var(CONFIRM_ENV) {
            Err(std::env::VarError::NotPresent) => Ok(Self::Interactive),
            Err(std::env::VarError::NotUnicode(_)) => Err(GwsError::Validation(format!(
                "{CONFIRM_ENV} is not valid UTF-8"
            ))),
            Ok(v) => Self::parse(&v),
        }
    }

    pub(crate) fn parse(v: &str) -> Result<Self, GwsError> {
        match v.trim().to_ascii_lowercase().as_str() {
            "" => Ok(Self::Interactive),
            "0" | "false" | "no" | "off" => Ok(Self::Off),
            "1" | "true" | "yes" | "on" => Ok(Self::Always),
            other => Err(GwsError::Validation(format!(
                "{CONFIRM_ENV} must be 1 or 0 (true/false), got {other:?}"
            ))),
        }
    }
}

pub(crate) fn is_destructive(method: &RestMethod) -> bool {
    method.http_method.eq_ignore_ascii_case("DELETE")
        || method
            .id
            .as_deref()
            .is_some_and(|id| PERMANENT_DELETE_METHODS.contains(&id))
}

/// Decide whether a destructive call may proceed.
///
/// `ask` shows the prompt and returns the user's answer; it is only called on
/// an interactive terminal.
pub(crate) fn confirm(
    description: &str,
    assume_yes: bool,
    policy: ConfirmPolicy,
    interactive: bool,
    ask: impl FnOnce(&str) -> Result<bool, GwsError>,
) -> Result<(), GwsError> {
    if assume_yes || policy == ConfirmPolicy::Off {
        return Ok(());
    }
    if interactive {
        let prompt = format!("{description} is destructive and cannot be undone. Proceed? [y/N] ");
        return if ask(&prompt)? {
            Ok(())
        } else {
            Err(GwsError::Validation(
                "Aborted: destructive operation not confirmed (pass --yes to skip this prompt)"
                    .to_string(),
            ))
        };
    }
    match policy {
        ConfirmPolicy::Always => Err(GwsError::Validation(format!(
            "{description} is destructive; pass --yes to confirm ({CONFIRM_ENV}=1 requires it in non-interactive runs)"
        ))),
        _ => Ok(()),
    }
}

/// Whether both stdin and stderr are terminals (a human can answer).
pub(crate) fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Prompt on stderr and read a line from stdin.
pub(crate) fn ask_on_terminal(prompt: &str) -> Result<bool, GwsError> {
    let mut err = std::io::stderr().lock();
    write!(err, "{prompt}")
        .and_then(|()| err.flush())
        .map_err(|e| GwsError::Validation(format!("failed to write confirmation prompt: {e}")))?;
    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| GwsError::Validation(format!("failed to read confirmation: {e}")))?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(http: &str, id: &str) -> RestMethod {
        RestMethod {
            http_method: http.to_string(),
            id: Some(id.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn destructive_detection() {
        assert!(is_destructive(&method("DELETE", "drive.files.delete")));
        assert!(is_destructive(&method(
            "POST",
            "gmail.users.messages.batchDelete"
        )));
        assert!(!is_destructive(&method("GET", "drive.files.list")));
        assert!(!is_destructive(&method(
            "POST",
            "gmail.users.messages.trash"
        )));
    }

    #[test]
    fn policy_parsing() {
        assert_eq!(ConfirmPolicy::parse("1").unwrap(), ConfirmPolicy::Always);
        assert_eq!(ConfirmPolicy::parse("false").unwrap(), ConfirmPolicy::Off);
        assert_eq!(
            ConfirmPolicy::parse("").unwrap(),
            ConfirmPolicy::Interactive
        );
        assert!(ConfirmPolicy::parse("maybe").is_err());
    }

    fn never_ask(_: &str) -> Result<bool, GwsError> {
        panic!("must not prompt")
    }

    #[test]
    fn gate_matrix() {
        use ConfirmPolicy::*;
        // --yes and Off never prompt.
        confirm("x", true, Always, true, never_ask).unwrap();
        confirm("x", false, Off, true, never_ask).unwrap();
        // Non-interactive: default runs, Always refuses.
        confirm("x", false, Interactive, false, never_ask).unwrap();
        let err = confirm("drive.files.delete", false, Always, false, never_ask).unwrap_err();
        assert!(err.to_string().contains("--yes"));
        // Interactive: answer decides.
        confirm("x", false, Interactive, true, |_| Ok(true)).unwrap();
        let err = confirm("x", false, Interactive, true, |_| Ok(false)).unwrap_err();
        assert!(err.to_string().contains("Aborted"));
    }
}
