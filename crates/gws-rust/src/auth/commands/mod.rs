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

//! `gwsr auth ...` subcommands.

mod export;
mod login;
mod logout;
mod picker;
mod profile;
mod status;

use crate::error::GwsError;

pub use login::run_login;

use super::profiles::GlobalOverrides;

/// `--profile` / `--impersonate`, accepted after `auth` too (they are
/// global options of the top-level command).
fn global_auth_args() -> [clap::Arg; 2] {
    [
        clap::Arg::new("profile")
            .long("profile")
            .global(true)
            .value_name("NAME")
            .help(
                "Credential profile (env: GWSR_PROFILE; default: `profile` in config.toml, \
                 set by `gwsr auth use`, then 'default')",
            ),
        clap::Arg::new("impersonate")
            .long("impersonate")
            .global(true)
            .value_name("EMAIL")
            .help(
                "Service accounts only: act as this user via domain-wide delegation \
                 (env: GWSR_IMPERSONATE)",
            ),
    ]
}

/// Merge a flag given before `auth` with the same flag given after it.
fn merge_flag(
    name: &str,
    before: Option<String>,
    after: Option<&String>,
) -> Result<Option<String>, GwsError> {
    match (before, after) {
        (Some(_), Some(_)) => Err(GwsError::Validation(format!(
            "--{name} was given more than once"
        ))),
        (Some(v), None) => Ok(Some(v)),
        (None, after) => Ok(after.cloned()),
    }
}

/// The `login` subcommand definition (shared with `auth setup --login`).
pub(crate) fn login_subcommand() -> clap::Command {
    clap::Command::new("login")
        .about("Sign in with OAuth (opens your browser; PKCE, loopback redirect)")
        .long_about(
            "Sign in with OAuth and store an encrypted refresh token in the active profile.\n\n\
             By default only read-only scopes for the core Workspace APIs are requested. Add \
             write access with --write or --full, or exact scopes with --scopes. Logins are \
             incremental: scopes granted earlier are kept.",
        )
        .arg(
            clap::Arg::new("services")
                .short('s')
                .long("services")
                .value_name("SERVICES")
                .help("Limit scopes to these services, comma-separated (e.g. drive,gmail,sheets)"),
        )
        .arg(
            clap::Arg::new("write")
                .long("write")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with_all(["full", "scopes"])
                .help("Request read-write scopes for the core services instead of read-only"),
        )
        .arg(
            clap::Arg::new("full")
                .long("full")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with_all(["write", "scopes"])
                .help("Request read-write core scopes plus Gmail settings, Pub/Sub and Cloud Platform"),
        )
        .arg(
            clap::Arg::new("scopes")
                .long("scopes")
                .value_name("SCOPES")
                .conflicts_with_all(["write", "full"])
                .help(
                    "Exact scopes, comma-separated; short names like gmail.modify are expanded \
                     to https://www.googleapis.com/auth/gmail.modify",
                ),
        )
        .arg(
            clap::Arg::new("no-browser")
                .long("no-browser")
                .action(clap::ArgAction::SetTrue)
                .help("Print the sign-in URL but do not open a browser"),
        )
        .arg(
            clap::Arg::new("no-localhost")
                .long("no-localhost")
                .action(clap::ArgAction::SetTrue)
                .help(
                    "Headless/SSH mode: open the URL on any machine, then paste the redirected \
                     URL back here (no local listener)",
                ),
        )
        .arg(
            clap::Arg::new("timeout")
                .long("timeout")
                .value_name("SECONDS")
                .value_parser(clap::value_parser!(u64).range(1..=3600))
                .default_value("300")
                .help("How long to wait for the browser sign-in"),
        )
        .arg(
            clap::Arg::new("login-hint")
                .long("login-hint")
                .value_name("EMAIL")
                .help("Pre-select this Google account on the consent screen"),
        )
}

/// The full `gwsr auth` command tree (for help, completions and man pages).
pub(crate) fn auth_command() -> clap::Command {
    clap::Command::new("auth")
        .bin_name("gwsr auth")
        .about("Manage authentication, profiles and credentials")
        .args(global_auth_args())
        .subcommand(login_subcommand())
        .subcommand(
            clap::Command::new("setup")
                .about("Create a GCP project + OAuth client with gcloud (interactive)")
                .disable_help_flag(true)
                .arg(
                    clap::Arg::new("args")
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true)
                        .num_args(0..)
                        .value_name("ARGS"),
                ),
        )
        .subcommand(
            clap::Command::new("status")
                .about("Show the active profile, credential source, key storage and scopes")
                .arg(
                    clap::Arg::new("offline")
                        .long("offline")
                        .action(clap::ArgAction::SetTrue)
                        .help("Do not contact Google to verify the credentials"),
                ),
        )
        .subcommand(
            clap::Command::new("export")
                .about("Show the profile's credentials with secrets masked, or write them unmasked to a file")
                .arg(
                    clap::Arg::new("unmasked")
                        .long("unmasked")
                        .action(clap::ArgAction::SetTrue)
                        .requires("output")
                        .help("Write the real secrets (requires --output and confirmation)"),
                )
                .arg(
                    clap::Arg::new("output")
                        .long("output")
                        .value_name("FILE")
                        .requires("unmasked")
                        .help("New file to write (created with mode 0600; must not exist)"),
                )
                .arg(
                    clap::Arg::new("confirm")
                        .long("yes-i-understand-this-exposes-secrets")
                        .action(clap::ArgAction::SetTrue)
                        .requires("unmasked")
                        .help("Skip the interactive confirmation (required when stdin is not a terminal)"),
                ),
        )
        .subcommand(
            clap::Command::new("logout")
                .about("Revoke the refresh token and delete the active profile's stored credentials")
                .arg(
                    clap::Arg::new("no-revoke")
                        .long("no-revoke")
                        .action(clap::ArgAction::SetTrue)
                        .help("Only delete local files; do not revoke the token at Google"),
                ),
        )
        .subcommand(clap::Command::new("list").about("List profiles"))
        .subcommand(
            clap::Command::new("use")
                .about("Make a profile the default for future commands")
                .arg(
                    clap::Arg::new("name")
                        .required(true)
                        .value_name("PROFILE"),
                ),
        )
}

/// Parse `args` with `cmd`; `Ok(None)` when clap printed help/version.
pub(crate) fn parse_or_help(
    cmd: clap::Command,
    name: &str,
    args: &[String],
) -> Result<Option<clap::ArgMatches>, GwsError> {
    match cmd.try_get_matches_from(std::iter::once(name.to_string()).chain(args.iter().cloned())) {
        Ok(m) => Ok(Some(m)),
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            e.print()
                .map_err(|io| GwsError::Validation(format!("failed to print help: {io}")))?;
            Ok(None)
        }
        Err(e) => Err(GwsError::Validation(e.to_string())),
    }
}

/// Handle `gwsr auth <subcommand>`.
///
/// # Errors
///
/// Argument errors and the subcommand's own failures.
pub async fn handle_auth_command(args: &[String], before: GlobalOverrides) -> Result<(), GwsError> {
    let Some(matches) = parse_or_help(auth_command(), "auth", args)? else {
        return Ok(());
    };
    let leaf = leaf_matches(&matches);
    super::profiles::set_global_overrides(GlobalOverrides {
        profile: merge_flag(
            "profile",
            before.profile,
            crate::args::value::<String>(leaf, "profile")?,
        )?,
        impersonate: merge_flag(
            "impersonate",
            before.impersonate,
            crate::args::value::<String>(leaf, "impersonate")?,
        )?,
    })?;
    match matches.subcommand() {
        Some(("login", m)) => login::handle(m).await,
        Some(("setup", m)) => {
            let setup_args: Vec<String> = crate::args::values(m, "args")?
                .map(|vals| vals.cloned().collect())
                .unwrap_or_default();
            super::setup::run_setup(&setup_args).await
        }
        Some(("status", m)) => status::handle(crate::args::flag(m, "offline")?).await,
        Some(("export", m)) => export::handle(m).await,
        Some(("logout", m)) => logout::handle(crate::args::flag(m, "no-revoke")?).await,
        Some(("list", _)) => profile::handle_list(),
        Some(("use", m)) => profile::handle_use(m),
        Some((other, _)) => Err(GwsError::Validation(format!(
            "unknown auth subcommand '{other}'"
        ))),
        None => {
            auth_command()
                .print_help()
                .map_err(|e| GwsError::Validation(format!("failed to print help: {e}")))?;
            Ok(())
        }
    }
}

/// The matches of the deepest subcommand (global args propagate there).
fn leaf_matches(m: &clap::ArgMatches) -> &clap::ArgMatches {
    match m.subcommand() {
        Some((_, sub)) => leaf_matches(sub),
        None => m,
    }
}

/// Convert an [`super::AuthError`] (or any error) into a CLI error.
pub(crate) fn auth_err(e: impl std::fmt::Display) -> GwsError {
    GwsError::Auth(e.to_string())
}

/// Print a command's JSON result to stdout in the configured format.
pub(crate) fn print_json(value: &serde_json::Value) -> Result<(), GwsError> {
    crate::formatter::emit_default(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tree_is_valid() {
        auth_command().debug_assert();
    }

    #[tokio::test]
    async fn help_and_empty_args_succeed() {
        handle_auth_command(&[], GlobalOverrides::default())
            .await
            .unwrap();
        handle_auth_command(&["--help".into()], GlobalOverrides::default())
            .await
            .unwrap();
        handle_auth_command(
            &["login".into(), "--help".into()],
            GlobalOverrides::default(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn unknown_subcommand_is_validation_error() {
        let err = handle_auth_command(&["frobnicate".into()], GlobalOverrides::default())
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)));
    }

    #[test]
    fn profile_flag_is_accepted_after_auth_and_merged_once() {
        let m = auth_command()
            .try_get_matches_from(["auth", "status", "--profile", "work"])
            .unwrap();
        assert_eq!(
            leaf_matches(&m)
                .get_one::<String>("profile")
                .map(String::as_str),
            Some("work")
        );
        let work = "work".to_string();
        assert_eq!(
            merge_flag("profile", None, Some(&work)).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(
            merge_flag("profile", Some("home".into()), None)
                .unwrap()
                .as_deref(),
            Some("home")
        );
        assert!(merge_flag("profile", Some("home".into()), Some(&work)).is_err());
    }

    #[test]
    fn export_unmasked_requires_output() {
        let r = auth_command().try_get_matches_from(["auth", "export", "--unmasked"]);
        assert!(r.is_err());
        let r = auth_command().try_get_matches_from(["auth", "export", "--output", "f"]);
        assert!(r.is_err());
        let r =
            auth_command().try_get_matches_from(["auth", "export", "--unmasked", "--output", "f"]);
        assert!(r.is_ok());
    }

    #[test]
    fn login_scope_flags_conflict() {
        let r = auth_command().try_get_matches_from(["auth", "login", "--write", "--full"]);
        assert!(r.is_err());
        let r = auth_command().try_get_matches_from(["auth", "login", "--timeout", "0"]);
        assert!(r.is_err());
        let r = auth_command().try_get_matches_from([
            "auth",
            "login",
            "-s",
            "gmail",
            "--write",
            "--no-browser",
        ]);
        assert!(r.is_ok());
    }
}
