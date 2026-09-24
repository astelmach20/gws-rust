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

//! Static command-line surface: the top-level `gwsr` command (clap derive)
//! and the global options shared with every dynamically built service
//! command.

use std::ffi::OsString;

use clap::builder::PossibleValuesParser;
use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::formatter::FORMAT_NAMES;
use crate::generate_skills::GenerateSkillsArgs;

/// Options accepted by every command, before or after the service name.
///
/// Argument ids (`format`, `sanitize`, `dry-run`, ...) are part of the
/// contract with helpers, which read them from `ArgMatches`.
#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Global options")]
pub struct GlobalArgs {
    /// Output format [default: json, or `format` in config.toml]
    #[arg(long, global = true, value_name = "FORMAT", value_parser = PossibleValuesParser::new(FORMAT_NAMES))]
    pub format: Option<String>,

    /// Filter output with a jq expression (string results print raw)
    #[arg(long, global = true, value_name = "EXPR")]
    pub jq: Option<String>,

    /// Columns for table/csv output, comma-separated (dot paths for nested fields)
    #[arg(long, global = true, value_name = "COLS", value_delimiter = ',')]
    pub columns: Option<Vec<String>>,

    /// Single-line JSON (default when stdout is not a terminal)
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub compact: bool,

    /// Indented JSON (default on a terminal)
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Validate the request locally and print it without sending it
    #[arg(id = "dry-run", long = "dry-run", global = true)]
    pub dry_run: bool,

    /// Screen API responses through a Model Armor template
    /// (projects/P/locations/L/templates/T; env: GWSR_SANITIZE_TEMPLATE)
    #[arg(long, global = true, value_name = "TEMPLATE")]
    pub sanitize: Option<String>,

    /// Override the service's API version (e.g. v2); also `<service>:<version>`
    #[arg(
        id = "api-version",
        long = "api-version",
        global = true,
        value_name = "VERSION"
    )]
    pub api_version: Option<String>,

    /// More diagnostics on stderr (-v info, -vv debug, -vvv trace)
    #[arg(short = 'v', long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// Only print errors on stderr
    #[arg(short = 'q', long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Credential profile (env: GWSR_PROFILE; default: `profile` in
    /// config.toml, set by `gwsr auth use`, then "default")
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Service accounts only: act as this user via domain-wide delegation
    /// (env: GWSR_IMPERSONATE)
    #[arg(long, global = true, value_name = "EMAIL")]
    pub impersonate: Option<String>,
}

impl GlobalArgs {
    /// The auth-related global options.
    pub fn auth_overrides(&self) -> crate::auth::profiles::GlobalOverrides {
        crate::auth::profiles::GlobalOverrides {
            profile: self.profile.clone(),
            impersonate: self.impersonate.clone(),
        }
    }
}

/// The global options as plain `clap::Arg`s, for commands built at runtime
/// from Discovery documents.
pub fn global_args() -> Vec<clap::Arg> {
    GlobalArgs::augment_args(clap::Command::new("globals"))
        .get_arguments()
        .cloned()
        .collect()
}

/// Shells supported by `gwsr completions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    /// GNU Bash
    Bash,
    /// Elvish
    Elvish,
    /// fish
    Fish,
    /// PowerShell
    Powershell,
    /// Z shell
    Zsh,
}

impl CompletionShell {
    /// Name understood by `clap_complete`'s environment completer.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Elvish => "elvish",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
            Self::Zsh => "zsh",
        }
    }
}

/// `gwsr` — Google Workspace from the command line.
#[derive(Debug, Parser)]
#[command(
    name = "gwsr",
    bin_name = "gwsr",
    version,
    about = "Google Workspace CLI: every Workspace API, built at runtime from Google's Discovery documents",
    long_about = None,
    arg_required_else_help = true,
    disable_help_subcommand = true,
    subcommand_value_name = "COMMAND|SERVICE",
    subcommand_help_heading = "Commands",
    after_help = crate::cli_args::after_help(),
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Option<TopCommand>,
}

/// Static top-level commands. Anything else is treated as a service name.
#[derive(Debug, Subcommand)]
pub enum TopCommand {
    /// Sign in, inspect and manage credentials (run `gwsr auth --help`)
    #[command(disable_help_flag = true)]
    Auth {
        /// Arguments passed to the auth subcommand
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },

    /// Show the request/response schema of a method or type
    ///
    /// Examples: `gwsr schema drive.files.list`, `gwsr schema drive.File --resolve-refs`
    Schema {
        /// `service.resource[.sub].method` or `service.Type`
        path: String,
        /// Inline referenced schemas
        #[arg(long)]
        resolve_refs: bool,
    },

    /// Inventory of services, resources, methods and helpers with their flags
    /// (JSON; `--format table|csv` gives one row per command)
    Commands {
        /// Limit the listing to these services (default: all)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,
    },

    /// Print a shell completion script (dynamic: completes services, resources
    /// and methods from cached Discovery documents)
    ///
    /// bash: `source <(gwsr completions bash)` ·
    /// zsh: `source <(gwsr completions zsh)` ·
    /// fish: `gwsr completions fish | source`
    Completions {
        /// Target shell
        shell: CompletionShell,
    },

    /// Send many API calls in multipart/mixed batch requests (NDJSON in,
    /// NDJSON out)
    Batch(crate::executor::batch::BatchArgs),

    /// Manage the local Discovery document cache
    Cache {
        #[command(subcommand)]
        action: CacheCommand,
    },

    /// Show help for a command or service (`gwsr help drive files list`)
    Help {
        /// Command path
        #[arg(num_args = 0.., value_name = "COMMAND")]
        path: Vec<String>,
    },

    /// Maintainer tools
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },

    /// A Google Workspace service (see SERVICES below)
    #[command(external_subcommand)]
    Service(Vec<OsString>),
}

/// `gwsr cache ...`
#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Delete every cached Discovery document
    Clear,
}

/// `gwsr dev ...`
#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Generate agent SKILL.md files from the command tree
    GenerateSkills(GenerateSkillsArgs),
    /// Generate man pages for the static commands
    Man {
        /// Directory to write `*.1` files into (created if missing)
        #[arg(long, value_name = "DIR", required = true)]
        output_dir: std::path::PathBuf,
    },
}

/// Services, environment and exit codes, appended to the top-level help.
pub fn after_help() -> String {
    let mut out = String::from("Services:\n");
    for entry in crate::services::SERVICES {
        let aliases = if entry.aliases.len() > 1 {
            format!(" (also: {})", entry.aliases[1..].join(", "))
        } else {
            String::new()
        };
        out.push_str(&format!(
            "  {:<18} {}{}\n",
            entry.aliases[0], entry.description, aliases
        ));
    }
    out.push_str(
        "\nUsage: gwsr <SERVICE> <RESOURCE> [SUB-RESOURCE] <METHOD> [--params JSON] [--json JSON]\n\
         Example: gwsr drive files list --params '{\"pageSize\": 5}' --format table\n",
    );
    out.push_str("\nEnvironment:\n");
    for var in crate::env::REGISTRY {
        out.push_str(&format!("  {:<24} {}\n", var.name, var.help));
    }
    out.push_str("\nExit codes:\n");
    for (code, desc) in crate::error::EXIT_CODE_DOCUMENTATION {
        out.push_str(&format!("  {code:<4} {desc}\n"));
    }
    out.push_str(
        "\nOutput is JSON by default (compact when piped); --format table|yaml|csv is opt-in.\n\
         Errors: one JSON object {\"error\":{...}} on stderr (human text with a human --format);\n\
         stdout stays empty on failure.\n\
         Config: ~/.config/gwsr/config.toml (flag > env > config > default).\n\
         This is not an officially supported Google product.",
    );
    out
}

/// Facts extracted from raw argv before full parsing: logging must start
/// before Discovery documents are fetched, and the API version selects which
/// document to fetch.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PreScan {
    /// `-v` count minus quiet (`-q` gives -1).
    pub verbosity: i8,
    /// `--api-version` value.
    pub api_version: Option<String>,
    /// `--format` value (used to pick the error format before full parsing).
    pub format: Option<String>,
}

/// Scan `args` (without argv\[0\]) up to a `--` terminator.
///
/// Only standalone `-v`/`-vv`/`--verbose`, `-q`/`--quiet` and
/// `--api-version[=]V` are recognised; clap still validates the full command
/// line afterwards.
pub fn prescan(args: &[OsString]) -> PreScan {
    let mut out = PreScan::default();
    let mut verbose: i8 = 0;
    let mut quiet = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else { continue };
        match arg {
            "--" => break,
            "--verbose" => verbose = verbose.saturating_add(1),
            "--quiet" | "-q" => quiet = true,
            "--api-version" => {
                out.api_version = iter.next().and_then(|v| v.to_str()).map(String::from)
            }
            a if a.starts_with("--api-version=") => {
                out.api_version = a.strip_prefix("--api-version=").map(String::from);
            }
            "--format" => out.format = iter.next().and_then(|v| v.to_str()).map(String::from),
            a if a.starts_with("--format=") => {
                out.format = a.strip_prefix("--format=").map(String::from);
            }
            a if a.len() > 1
                && a.starts_with('-')
                && !a.starts_with("--")
                && a[1..].chars().all(|c| c == 'v') =>
            {
                // Saturating: `-vvvv…` beyond i8::MAX is simply maximal.
                let n = i8::try_from(a.len() - 1).unwrap_or(i8::MAX);
                verbose = verbose.saturating_add(n);
            }
            _ => {}
        }
    }
    out.verbosity = if quiet { -1 } else { verbose };
    out
}

/// The static command tree (used for help, man pages and completion).
pub fn command() -> clap::Command {
    Cli::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn global_args_keep_helper_contract_ids() {
        let ids: Vec<String> = global_args()
            .iter()
            .map(|a| a.get_id().to_string())
            .collect();
        for id in [
            "format",
            "sanitize",
            "dry-run",
            "jq",
            "columns",
            "compact",
            "pretty",
            "verbose",
            "quiet",
            "api-version",
        ] {
            assert!(ids.contains(&id.to_string()), "missing {id} in {ids:?}");
        }
        assert!(global_args().iter().all(|a| a.is_global_set()));
        assert!(
            global_args()
                .iter()
                .all(|a| a.get_help_heading() == Some("Global options"))
        );
    }

    #[test]
    fn format_is_validated_by_clap() {
        let cmd = clap::Command::new("t").args(global_args());
        let err = cmd
            .clone()
            .try_get_matches_from(["t", "--format", "xml"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        let m = cmd.try_get_matches_from(["t", "--format", "yaml"]).unwrap();
        assert_eq!(
            m.get_one::<String>("format").map(String::as_str),
            Some("yaml")
        );
    }

    #[test]
    fn columns_split_on_commas() {
        let m = clap::Command::new("t")
            .args(global_args())
            .try_get_matches_from(["t", "--columns", "id,name"])
            .unwrap();
        let cols: Vec<&String> = m.get_many::<String>("columns").unwrap().collect();
        assert_eq!(cols, ["id", "name"]);
    }

    #[test]
    fn prescan_counts_verbosity_and_finds_api_version() {
        assert_eq!(
            prescan(&os(&[
                "drive",
                "-vv",
                "files",
                "--verbose",
                "--api-version",
                "v2"
            ])),
            PreScan {
                verbosity: 3,
                api_version: Some("v2".into()),
                format: None,
            }
        );
        assert_eq!(prescan(&os(&["-q", "drive"])).verbosity, -1);
        assert_eq!(
            prescan(&os(&["drive", "--api-version=v2"]))
                .api_version
                .as_deref(),
            Some("v2")
        );
        assert_eq!(
            prescan(&os(&["drive", "--format", "table"]))
                .format
                .as_deref(),
            Some("table")
        );
        assert_eq!(
            prescan(&os(&["--format=csv"])).format.as_deref(),
            Some("csv")
        );
        // Stops at `--` and ignores look-alikes.
        assert_eq!(prescan(&os(&["x", "--", "-v"])).verbosity, 0);
        assert_eq!(prescan(&os(&["-vx"])).verbosity, 0);
    }
}
