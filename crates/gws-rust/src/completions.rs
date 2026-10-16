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
    CompleteEnv::with_factory(move || with_auth_tree(completion_command(&words)))
        .var(COMPLETE_VAR)
        .bin("gwsr")
        .try_complete(args, cwd.as_deref())
}

/// Replace the top-level `auth` passthrough argument with the real `gwsr auth`
/// tree, which `auth` parses on its own, so completion, man pages and the
/// `gwsr commands` inventory cover its subcommands and flags. The top-level
/// description is kept.
///
/// `auth setup` is a passthrough inside that tree as well (it parses its own
/// flags), so it is replaced by the command `setup` actually parses with.
pub(crate) fn with_auth_tree(root: Command) -> Command {
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

    /// The Discovery fixture for `alias`, or an empty document of the right
    /// name (helper-only and fixture-less services still get their helpers).
    fn fixture_or_empty(alias: &str) -> RestDescription {
        let (api, version) = crate::services::resolve_service(alias).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/discovery/{api}_{version}.json"));
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap(),
            Err(_) => RestDescription {
                name: api,
                version,
                ..Default::default()
            },
        }
    }

    /// Every command completion can reach: the static tree, the `auth` tree,
    /// and each service with its helpers and generated methods.
    fn full_completion_tree() -> Command {
        let mut root = with_auth_tree(crate::cli_args::command());
        for entry in SERVICES {
            let alias = entry.aliases[0];
            root = root.subcommand(crate::service::build_command(
                alias,
                &fixture_or_empty(alias),
            ));
        }
        root
    }

    /// Ids of arguments named PATH that hold something other than a local path.
    const NOT_LOCAL_PATHS: &[&str] = &[
        // `gwsr schema drive.files.list`: a Discovery method or type path.
        "path",
        // `admin +user-create --org-unit /Sales`: an organizational unit path.
        "org-unit",
    ];

    fn assert_path_hints(cmd: &Command, at: &str, checked: &mut usize) {
        for arg in cmd.get_arguments() {
            let names = arg.get_value_names().unwrap_or_default();
            let names_a_path = names
                .iter()
                .any(|n| matches!(n.as_str(), "PATH" | "FILE" | "DIR"));
            if !names_a_path || NOT_LOCAL_PATHS.contains(&arg.get_id().as_str()) {
                continue;
            }
            *checked += 1;
            assert!(
                matches!(
                    arg.get_value_hint(),
                    clap::ValueHint::AnyPath | clap::ValueHint::FilePath | clap::ValueHint::DirPath
                ),
                "{at} --{} takes a local {:?} but completes no paths",
                arg.get_id(),
                names
            );
        }
        for sub in cmd.get_subcommands() {
            assert_path_hints(sub, &format!("{at} {}", sub.get_name()), checked);
        }
    }

    #[test]
    fn every_local_path_flag_completes_paths() {
        let mut checked = 0;
        assert_path_hints(&full_completion_tree(), "gwsr", &mut checked);
        // --upload/--output on generated methods, helper files and
        // directories, batch --input, auth export, dev commands.
        assert!(checked > 20, "only {checked} path flags were found");
    }

    fn complete_values(ws: &[&str], cwd: &Path) -> Vec<String> {
        let words = words(ws);
        let mut cmd = with_auth_tree(completion_tree(&words, &|api, version| {
            let doc = fixture_or_empty(api);
            (doc.version == version).then_some(doc)
        }));
        clap_complete::engine::complete(&mut cmd, words.clone(), words.len() - 1, Some(cwd))
            .unwrap()
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn path_flags_complete_files_and_directories() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("report.pdf"), "x").unwrap();
        std::fs::create_dir(tmp.path().join("reports")).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        let both = vec!["report.pdf".to_string(), format!("reports{sep}")];

        // A helper's input file, and a generated method's --upload/--output.
        assert_eq!(
            complete_values(&["gwsr", "drive", "+upload", "--file", "rep"], tmp.path()),
            both
        );
        assert_eq!(
            complete_values(
                &["gwsr", "drive", "files", "create", "--upload", "rep"],
                tmp.path()
            ),
            both
        );
        assert_eq!(
            complete_values(&["gwsr", "drive", "files", "get", "-o", "rep"], tmp.path()),
            both
        );
        // A directory flag offers only directories.
        assert_eq!(
            complete_values(&["gwsr", "drive", "+sync", "--dir", "rep"], tmp.path()),
            vec![format!("reports{sep}")]
        );
        // An org-unit path is not a local path.
        assert!(
            complete_values(
                &["gwsr", "admin", "+user-create", "--org-unit", "rep"],
                tmp.path()
            )
            .is_empty()
        );
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
