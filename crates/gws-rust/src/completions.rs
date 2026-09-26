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
use crate::discovery::RestDescription;
use crate::error::GwsError;
use crate::services::SERVICES;

/// Environment variable that activates completion mode.
pub const COMPLETE_VAR: &str = "GWSR_COMPLETE";

/// Whether the global option spelled `word` (`--name` or `-c`, without an
/// inline `=value`) consumes the next word as its value.
fn global_takes_value(globals: &[clap::Arg], word: &str) -> bool {
    if word.contains('=') {
        return false;
    }
    globals.iter().any(|arg| {
        let named = match word.strip_prefix("--") {
            Some(long) => arg.get_long() == Some(long),
            None => word
                .strip_prefix('-')
                .and_then(|s| {
                    let mut chars = s.chars();
                    chars.next().filter(|_| chars.next().is_none())
                })
                .is_some_and(|c| arg.get_short() == Some(c)),
        };
        named && arg.get_action().takes_values()
    })
}

/// The service token being completed, as typed (`drive`, `drive:v2`,
/// `youtube:v3`): the first positional word after the binary name, skipping
/// global options and their values. `None` for a static command.
fn service_token(words: &[OsString]) -> Option<&str> {
    let globals = crate::cli_args::global_args();
    let static_tree = crate::cli_args::command();
    let mut iter = words.iter().skip(1);
    while let Some(word) = iter.next() {
        let word = word.to_str()?;
        if word == "--" {
            return None;
        }
        if word.starts_with('-') {
            if global_takes_value(&globals, word) {
                iter.next();
            }
            continue;
        }
        return static_tree.find_subcommand(word).is_none().then_some(word);
    }
    None
}

/// The registered service a token names (`drive` or `drive:v2`), if any.
fn service_word(words: &[OsString]) -> Option<&'static crate::services::ServiceEntry> {
    let token = service_token(words)?;
    let name = token.split_once(':').map_or(token, |(name, _)| name);
    crate::services::find_service(name)
}

/// The Discovery document to complete the typed service from: the same API
/// and version the command itself would load (`--api-version` beats the
/// `:version` suffix, which beats the registry default). `None` when the
/// token or version is invalid, or nothing is cached.
fn active_document(
    words: &[OsString],
    token: &str,
    load_cached: LoadCached<'_>,
) -> Option<RestDescription> {
    let prescan = crate::cli_args::prescan(words.get(1..)?);
    // An invalid spec or version is reported when the command runs; here it
    // just means there is nothing to complete.
    let (_, api, version) =
        crate::parse_service_and_version(token, prescan.api_version.as_deref()).ok()?;
    crate::service::synthetic_document(&api).or_else(|| load_cached(&api, &version))
}

/// Static tree plus one subcommand per service. The service being completed
/// gets its full tree from the Discovery cache when available.
pub fn completion_command(words: &[OsString]) -> Command {
    completion_tree(words, &|api, version| {
        // Completion must never fail loudly inside the shell: an unreadable
        // cache just means no resource completions.
        crate::discovery::loader()
            .ok()
            .and_then(|l| l.load_cached(api, version).ok())
            .flatten()
    })
}

/// A cached-document lookup by `(api_name, version)`.
type LoadCached<'a> = &'a dyn Fn(&str, &str) -> Option<RestDescription>;

fn completion_tree(words: &[OsString], load_cached: LoadCached<'_>) -> Command {
    let token = service_token(words);
    let active = service_word(words);
    let doc = token.and_then(|t| active_document(words, t, load_cached));
    // A `<api>:<version>` token is its own subcommand, so clap can walk into it.
    let versioned = token.filter(|t| t.contains(':'));
    let mut root = crate::cli_args::command();
    for entry in SERVICES {
        let alias = entry.aliases[0];
        let is_active = versioned.is_none() && active.is_some_and(|a| a.aliases[0] == alias);
        let sub = match &doc {
            Some(doc) if is_active => crate::service::build_command(alias, doc),
            _ => Command::new(alias),
        };
        let sub = sub
            .about(entry.description)
            .aliases(entry.aliases[1..].iter().copied());
        root = root.subcommand(sub);
    }
    if let Some(token) = versioned {
        let sub = match &doc {
            Some(doc) => crate::service::build_command(token, doc),
            None => Command::new(token.to_string()),
        };
        root = root.subcommand(sub.about(active.map_or("", |e| e.description)));
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
    CompleteEnv::with_factory(move || completion_command(&words))
        .var(COMPLETE_VAR)
        .bin("gwsr")
        .try_complete(args, cwd.as_deref())
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
    let mut root = crate::cli_args::command();
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
        // A static command's arguments are not a service.
        assert!(service_token(&words(&["gwsr", "schema", "drive:v2.files.list"])).is_none());
        // Values of global options are skipped.
        assert_eq!(
            service_token(&words(&[
                "gwsr",
                "--profile",
                "work",
                "--format=csv",
                "drive:v2"
            ])),
            Some("drive:v2")
        );
        assert_eq!(
            service_word(&words(&["gwsr", "--jq", ".x", "-v", "gmail"])).map(|e| e.aliases[0]),
            Some("gmail")
        );
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

    /// A cache holding only `drive` `v2`, whose one resource is `revisionsV2`.
    fn drive_v2_only(api: &str, version: &str) -> Option<RestDescription> {
        (api == "drive" && version == "v2").then(|| {
            let mut res = crate::discovery::RestResource::default();
            res.methods.insert(
                "get".into(),
                crate::discovery::RestMethod {
                    http_method: "GET".into(),
                    ..Default::default()
                },
            );
            let mut doc = RestDescription {
                name: "drive".into(),
                version: "v2".into(),
                ..Default::default()
            };
            doc.resources.insert("revisionsV2".into(), res);
            doc
        })
    }

    fn has_resource(cmd: &Command, service: &str, resource: &str) -> bool {
        cmd.find_subcommand(service)
            .is_some_and(|s| s.find_subcommand(resource).is_some())
    }

    #[test]
    fn api_version_flag_selects_the_cached_document() {
        for ws in [
            &["gwsr", "drive", "--api-version", "v2", ""][..],
            &["gwsr", "--api-version=v2", "drive", ""],
            &["gwsr", "drive:v1", "--api-version", "v2", ""],
        ] {
            let cmd = completion_tree(&words(ws), &drive_v2_only);
            let typed = ws.iter().find(|w| w.starts_with("drive")).unwrap();
            assert!(has_resource(&cmd, typed, "revisionsV2"), "{ws:?}");
        }
    }

    #[test]
    fn service_version_suffix_completes_that_version() {
        let cmd = completion_tree(&words(&["gwsr", "drive:v2", ""]), &drive_v2_only);
        assert!(has_resource(&cmd, "drive:v2", "revisionsV2"));
        // Any `<api>:<version>`, registered or not, is completed the same way.
        let cmd = completion_tree(&words(&["gwsr", "drive:v3", ""]), &drive_v2_only);
        assert!(cmd.find_subcommand("drive:v3").is_some());
        assert!(!has_resource(&cmd, "drive:v3", "revisionsV2"));
    }

    #[test]
    fn default_version_is_used_without_an_override() {
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |api: &str, version: &str| {
            seen.borrow_mut().push(format!("{api}:{version}"));
            None
        };
        completion_tree(&words(&["gwsr", "drive", ""]), &record);
        assert_eq!(*seen.borrow(), vec!["drive:v3".to_string()]);
    }

    #[test]
    fn invalid_version_completes_nothing_and_does_not_fail() {
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |api: &str, version: &str| {
            seen.borrow_mut().push(format!("{api}:{version}"));
            None
        };
        for ws in [
            &["gwsr", "drive", "--api-version", "../x", ""][..],
            &["gwsr", "drive:", ""],
        ] {
            let cmd = completion_tree(&words(ws), &record);
            assert!(cmd.find_subcommand("drive").is_some(), "{ws:?}");
        }
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
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
