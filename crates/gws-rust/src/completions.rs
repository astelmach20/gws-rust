// Copyright 2026 The gws-rust Authors
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

//! Shell completion and man pages.
//!
//! Completion is dynamic (`clap_complete`'s environment protocol): the script
//! printed by `gwsr completions <shell>` calls back into `gwsr` with
//! `GWSR_COMPLETE=<shell>` set on every `<TAB>`. The callback completes static
//! commands, service names and, from cached Discovery documents only (never
//! the network), the resources, methods, helpers and flags of the service
//! being typed.

use std::ffi::OsString;
use std::path::Path;

use clap::Command;
use clap_complete::env::{CompleteEnv, Shells};

use crate::cli_args::CompletionShell;
use crate::error::GwsError;
use crate::services::SERVICES;

/// Environment variable that activates completion mode.
pub const COMPLETE_VAR: &str = "GWSR_COMPLETE";

/// The first word after the binary name that names a known service.
fn service_word(words: &[OsString]) -> Option<&'static crate::services::ServiceEntry> {
    words
        .iter()
        .skip(1)
        .filter_map(|w| w.to_str())
        .filter(|w| !w.starts_with('-'))
        .find_map(|w| SERVICES.iter().find(|e| e.aliases.contains(&w)))
}

/// Static tree plus one subcommand per service. The service being completed
/// gets its full tree from the Discovery cache when available.
pub fn completion_command(words: &[OsString]) -> Command {
    let active = service_word(words);
    let mut root = crate::cli_args::command();
    for entry in SERVICES {
        let alias = entry.aliases[0];
        let is_active = active.is_some_and(|a| a.aliases[0] == alias);
        let cached = if is_active {
            crate::service::synthetic_document(entry.api_name).or_else(|| {
                // Completion must never fail loudly inside the shell: an
                // unreadable cache just means no resource completions.
                crate::discovery::loader()
                    .ok()
                    .and_then(|l| l.load_cached(entry.api_name, entry.version).ok())
                    .flatten()
            })
        } else {
            None
        };
        let sub = match cached {
            Some(doc) => crate::service::build_command(alias, &doc),
            None => Command::new(alias),
        };
        let sub = sub
            .about(entry.description)
            .aliases(entry.aliases[1..].iter().copied());
        root = root.subcommand(sub);
    }
    root
}

/// Handle a completion callback if `GWSR_COMPLETE` is set.
/// Returns `Ok(true)` when the request was handled and the process should exit.
pub fn complete_if_requested() -> Result<bool, clap::Error> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let words: Vec<OsString> = match args.iter().position(|a| a == "--") {
        Some(i) => args[i + 1..].to_vec(),
        None => Vec::new(),
    };
    // The working directory only seeds file-path completion; when it cannot
    // be read, clap completes paths without it (a completion script has no
    // channel to report an error to the user's shell prompt).
    let cwd = std::env::current_dir().ok();
    CompleteEnv::with_factory(move || with_auth_tree(completion_command(&words)))
        .var(COMPLETE_VAR)
        .bin("gwsr")
        .try_complete(args, cwd.as_deref())
}

/// Replace the top-level `auth` passthrough argument with the real `gwsr auth`
/// tree, which `auth` parses on its own, so completion and man pages cover its
/// subcommands and flags. The top-level description is kept.
///
/// `auth setup` is a passthrough inside that tree as well (it parses its own
/// flags), so it is replaced by the command `setup` actually parses with.
fn with_auth_tree(root: Command) -> Command {
    root.mut_subcommands(|sub| {
        if sub.get_name() != "auth" {
            return sub;
        }
        let auth = crate::auth::commands::auth_command()
            .mut_subcommand("setup", |_| crate::auth::setup::setup_command());
        match sub.get_about() {
            Some(about) => auth.about(about.clone()),
            None => auth,
        }
    })
}

/// Print the registration script for `shell`.
pub fn registration_script(shell: CompletionShell, completer: &str) -> Result<String, GwsError> {
    let shells = Shells::builtins();
    let completer_impl = shells
        .completer(shell.name())
        .ok_or_else(|| GwsError::Validation(format!("unsupported shell '{}'", shell.name())))?;
    let mut buf = Vec::new();
    completer_impl
        .write_registration(COMPLETE_VAR, "gwsr", "gwsr", completer, &mut buf)
        .map_err(|e| {
            GwsError::from(anyhow::Error::new(e).context("failed to render the completion script"))
        })?;
    String::from_utf8(buf).map_err(|e| {
        GwsError::from(anyhow::Error::new(e).context("completion script is not UTF-8"))
    })
}

/// Write man pages for the static command tree into `dir`.
/// Returns the files written.
///
/// With `dry_run` every page is still rendered (so failures surface) but
/// nothing is created or written; the returned paths are the pages that
/// would be written.
pub fn write_man_pages(dir: &Path, dry_run: bool) -> Result<Vec<std::path::PathBuf>, GwsError> {
    if !dry_run {
        std::fs::create_dir_all(dir).map_err(|e| {
            GwsError::from(
                anyhow::Error::new(e).context(format!("failed to create {}", dir.display())),
            )
        })?;
    }
    let mut root = with_auth_tree(crate::cli_args::command());
    root.build();
    let mut written = Vec::new();
    write_page(&root, "gwsr", dir, dry_run, &mut written)?;
    Ok(written)
}

fn write_page(
    cmd: &Command,
    name: &str,
    dir: &Path,
    dry_run: bool,
    written: &mut Vec<std::path::PathBuf>,
) -> Result<(), GwsError> {
    let path = dir.join(format!("{name}.1"));
    let mut buf = Vec::new();
    clap_mangen::Man::new(cmd.clone().name(name.to_string()))
        .render(&mut buf)
        .map_err(|e| GwsError::from(anyhow::Error::new(e).context("failed to render man page")))?;
    if !dry_run {
        std::fs::write(&path, buf).map_err(|e| {
            GwsError::from(
                anyhow::Error::new(e).context(format!("failed to write {}", path.display())),
            )
        })?;
    }
    written.push(path);
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() || sub.get_name() == "help" {
            continue;
        }
        write_page(
            sub,
            &format!("{name}-{}", sub.get_name()),
            dir,
            dry_run,
            written,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[&str]) -> Vec<OsString> {
        ws.iter().map(OsString::from).collect()
    }

    #[test]
    fn service_word_skips_flags_and_binary() {
        assert_eq!(
            service_word(&words(&["gwsr", "-v", "drive", "files"])).map(|e| e.aliases[0]),
            Some("drive")
        );
        assert!(service_word(&words(&["gwsr", "schema"])).is_none());
        assert!(
            service_word(&words(&["drive"])).is_none(),
            "argv[0] is not a service"
        );
    }

    #[test]
    fn completion_tree_lists_every_service_and_static_command() {
        let cmd = completion_command(&words(&["gwsr"]));
        for entry in SERVICES {
            assert!(
                cmd.find_subcommand(entry.aliases[0]).is_some(),
                "{}",
                entry.aliases[0]
            );
        }
        for name in ["auth", "schema", "commands", "completions", "cache", "dev"] {
            assert!(cmd.find_subcommand(name).is_some(), "{name}");
        }
    }

    #[test]
    fn auth_tree_replaces_the_passthrough_and_stays_valid() {
        let cmd = with_auth_tree(completion_command(&words(&["gwsr", "auth"])));
        cmd.clone().debug_assert();
        let auth = cmd.find_subcommand("auth").unwrap();
        for sub in [
            "login", "status", "list", "use", "export", "logout", "setup",
        ] {
            assert!(auth.find_subcommand(sub).is_some(), "{sub}");
        }
        assert!(
            auth.get_arguments().all(|a| a.get_id() != "args"),
            "the passthrough argument is gone"
        );
        let setup = auth.find_subcommand("setup").unwrap();
        assert!(
            setup.get_arguments().all(|a| a.get_id() != "args"),
            "setup's passthrough argument is gone"
        );
        for flag in ["project", "login", "dry-run", "non-interactive"] {
            assert!(
                setup.get_arguments().any(|a| a.get_long() == Some(flag)),
                "{flag}"
            );
        }
        let top = crate::cli_args::command();
        assert_eq!(
            auth.get_about().map(ToString::to_string),
            top.find_subcommand("auth")
                .unwrap()
                .get_about()
                .map(ToString::to_string)
        );
    }

    #[test]
    fn synthetic_service_completes_its_helpers_without_cache() {
        let cmd = completion_command(&words(&["gwsr", "workflow", ""]));
        let wf = cmd.find_subcommand("workflow").unwrap();
        assert!(wf.get_subcommands().any(|s| s.get_name().starts_with('+')));
    }

    #[test]
    fn registration_scripts_reference_the_env_var() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
            CompletionShell::Powershell,
            CompletionShell::Elvish,
        ] {
            let script = registration_script(shell, "gwsr").unwrap();
            assert!(script.contains(COMPLETE_VAR), "{shell:?}: {script}");
        }
    }

    #[test]
    fn man_pages_are_written_for_static_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let files = write_man_pages(tmp.path(), false).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for n in [
            "gwsr.1",
            "gwsr-schema.1",
            "gwsr-cache-clear.1",
            "gwsr-dev-generate-skills.1",
        ] {
            assert!(names.contains(&n.to_string()), "{n} missing from {names:?}");
        }
        let page = std::fs::read_to_string(tmp.path().join("gwsr.1")).unwrap();
        assert!(page.contains(".TH"));
    }

    #[test]
    fn man_pages_dry_run_lists_pages_but_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("man");
        let dry = write_man_pages(&out, true).unwrap();
        assert!(
            !out.exists(),
            "dry run must not create the output directory"
        );
        let real = write_man_pages(&out, false).unwrap();
        assert_eq!(
            dry, real,
            "a dry run reports exactly the pages a real run writes"
        );
    }
}
