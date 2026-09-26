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

//! Google Workspace CLI (gwsr)
//!
//! A dynamic, schema-driven CLI for Google Workspace APIs. Static commands
//! (`auth`, `schema`, `commands`, `completions`, `cache`, `dev`) are defined
//! with clap derive in [`cli_args`]; every other first word is a service
//! whose command tree is built at runtime from its Discovery document.

#![cfg_attr(not(test), forbid(unsafe_code))]

mod args;
mod auth;
mod cli_args;
mod client;
mod commands;
mod completions;
mod config;
mod confirm;
mod discovery;
mod env;
mod error;
mod executor;
mod formatter;
mod fs_util;
mod generate_skills;
mod hardening;
mod helpers;
mod inventory;
mod jq;
mod logging;
mod output;
mod output_file;
mod schema;
mod service;
mod services;
mod text;
mod timezone;
mod transport;
pub(crate) mod validate;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{FromArgMatches, Parser};

use cli_args::{CacheCommand, Cli, DevCommand, GlobalArgs, PreScan, TopCommand};
use config::{JsonStylePref, Settings};
use error::{CliError, ErrorContext, GwsError};
use formatter::{JsonStyle, OutputFormat, OutputSettings};

fn main() -> ExitCode {
    // Shell completion callbacks must run before anything writes to stdout.
    match completions::complete_if_requested() {
        Ok(true) => return ExitCode::SUCCESS,
        Ok(false) => {}
        Err(e) => return finish(Err(CliError::from(e)), false),
    }

    install_broken_pipe_hook();
    let hardening_warnings = hardening::apply();

    let args: Vec<OsString> = std::env::args_os().collect();
    let prescan = cli_args::prescan(args.get(1..).unwrap_or_default());

    // Every environment variable is validated here, before anything uses
    // one and whether or not the command needs it. An invalid value or an
    // unknown GWSR_* name is a configuration error (exit 8).
    let env = match env::get() {
        Ok(env) => env,
        Err(e) => return finish(Err(e.into()), human_output(prescan.format.as_deref(), None)),
    };
    // Before the config file is read, the error format comes from the flag or env.
    let early_human = human_output(prescan.format.as_deref(), env.format);

    let cli = match parse_top_level(&args) {
        Ok(Some(cli)) => cli,
        Ok(None) => return finish(Ok(()), early_human),
        Err(e) => return finish(Err(e), early_human),
    };

    let settings = match config::load() {
        Ok(s) => s,
        Err(e) => return finish(Err(e.into()), early_human),
    };
    let human = human_output(prescan.format.as_deref(), Some(settings.format));
    if let Err(e) = gws_rust_core::client::install_request_timeout(settings.request_timeout) {
        return finish(Err(e.into()), human);
    }

    let mut log_opts = logging::LogOptions::from_env(prescan.verbosity, env);
    log_opts.config_log = settings.log.clone();
    log_opts.human = human || prescan.verbosity > 0;
    log_opts.file_dir = settings.log_file.clone();
    let log_guard = match logging::init(&log_opts) {
        Ok(guard) => guard,
        Err(e) => return finish(Err(e.into()), human),
    };
    for warning in hardening_warnings {
        tracing::warn!("{warning}");
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return finish(
                Err(GwsError::from(
                    anyhow::Error::new(e).context("failed to start the async runtime"),
                )
                .into()),
                human,
            );
        }
    };

    tracing::debug!(?settings, "resolved settings");
    let result = runtime.block_on(dispatch(cli, &args, &settings, &prescan));
    // Stop background tasks, then flush file logs before exiting.
    drop(runtime);
    let code = finish(result, human);
    drop(log_guard);
    code
}

/// True when the effective output format is a human one (not JSON):
/// `--format` flag > `fallback` (the validated `GWSR_FORMAT`, or the resolved
/// settings once the config file is loaded) > JSON.
///
/// This only picks the form of an error report, possibly before the flag is
/// validated, so an unparseable flag is skipped here on purpose: clap rejects
/// it and that error is then reported (as JSON, the default).
fn human_output(flag: Option<&str>, fallback: Option<OutputFormat>) -> bool {
    let from_flag = flag.and_then(|f| OutputFormat::parse(f).ok());
    let format = from_flag.or(fallback).unwrap_or_default();
    format != OutputFormat::Json
}

/// Map the outcome to an exit code, reporting errors per the output contract
/// in [`error`]. A closed stdout (`gwsr ... | head`) is a clean exit.
fn finish(result: Result<(), CliError>, human: bool) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) if output::stdout_closed() => ExitCode::SUCCESS,
        Err(e) => {
            tracing::debug!(error = ?e, "command failed");
            error::report(&e, human);
            // Exit codes are 1..=10; the saturation is unreachable.
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(u8::MAX))
        }
    }
}

/// Safety net for code that still prints with `println!`, which panics when
/// the reader closes the pipe (`gwsr ... | head`). All output written through
/// [`output::emit`] handles EPIPE without panicking; any remaining
/// `println!` site exits quietly with status 0 here instead of reporting a
/// panic. Every other panic goes to the default hook.
fn install_broken_pipe_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("");
        if is_stdout_broken_pipe_panic(message) {
            std::process::exit(0);
        }
        default_hook(info);
    }));
}

fn is_stdout_broken_pipe_panic(message: &str) -> bool {
    message.starts_with("failed printing to stdout")
        && (message.contains("Broken pipe")
            || message.contains("os error 32)")
            || message.contains("os error 232)"))
}

/// True for clap "errors" that are really requests to print help/version.
fn is_display_request(e: &clap::Error) -> bool {
    use clap::error::ErrorKind;
    matches!(
        e.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

/// Print help/version text to stdout (exit status 0).
fn print_display(e: &clap::Error) -> Result<(), CliError> {
    let styled = e.render();
    let text = if output::stdout_is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        styled.ansi().to_string()
    } else {
        styled.to_string()
    };
    output::emit(text.trim_end()).map_err(CliError::from)
}

/// Parse the static top-level command line. `Ok(None)` means help or version
/// was printed. `gwsr help <path>` is rewritten to `gwsr <path> --help`.
fn parse_top_level(args: &[OsString]) -> Result<Option<Cli>, CliError> {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) if is_display_request(&e) => {
            print_display(&e)?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let Some(TopCommand::Help { path }) = &cli.command else {
        return Ok(Some(cli));
    };
    let bin = args
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from("gwsr"));
    let mut rewritten: Vec<OsString> = vec![bin];
    if path.first().is_some_and(|p| p != "help") {
        rewritten.extend(path.iter().map(OsString::from));
    }
    rewritten.push(OsString::from("--help"));
    match Cli::try_parse_from(&rewritten) {
        Err(e) if is_display_request(&e) => {
            print_display(&e)?;
            Ok(None)
        }
        Err(e) => Err(e.into()),
        // `auth` and services handle `--help` themselves.
        Ok(cli) => Ok(Some(cli)),
    }
}

/// Install output settings from global flags, falling back to env/config.
fn install_output(global: &GlobalArgs, settings: &Settings) -> Result<(), CliError> {
    let terminal = output::stdout_is_terminal();
    let json_style = if global.compact {
        JsonStyle::Compact
    } else if global.pretty {
        JsonStyle::Pretty
    } else {
        match settings.json_style {
            JsonStylePref::Compact => JsonStyle::Compact,
            JsonStylePref::Pretty => JsonStyle::Pretty,
            JsonStylePref::Auto if terminal => JsonStyle::Pretty,
            JsonStylePref::Auto => JsonStyle::Compact,
        }
    };
    let jq = global
        .jq
        .as_deref()
        .map(jq::JqFilter::compile)
        .transpose()?;
    formatter::install_settings(OutputSettings {
        json_style,
        default_format: effective_format(global, settings)?,
        columns: global.columns.clone(),
        jq,
        terminal,
    })?;
    Ok(())
}

/// The effective format for static commands (flag > env/config).
fn effective_format(global: &GlobalArgs, settings: &Settings) -> Result<OutputFormat, GwsError> {
    match global.format.as_deref() {
        Some(f) => OutputFormat::from_str(f),
        None => Ok(settings.format),
    }
}

/// Model Armor settings: `--sanitize` > `GWSR_SANITIZE_TEMPLATE` > config.
fn sanitize_config(
    global: &GlobalArgs,
    settings: &Settings,
) -> Result<helpers::modelarmor::SanitizeConfig, GwsError> {
    let template = global
        .sanitize
        .clone()
        .or_else(|| settings.sanitize_template.clone());
    // SEC-05: a malformed template fails before any request is made.
    if let Some(t) = &template {
        helpers::modelarmor::ModelArmorTemplate::parse(t)?;
    }
    Ok(helpers::modelarmor::SanitizeConfig {
        template,
        mode: settings.sanitize_mode.clone(),
    })
}

fn emit_value(value: &serde_json::Value, format: OutputFormat) -> Result<(), CliError> {
    let text = formatter::format_value(value, &format)?;
    output::emit(&text)?;
    Ok(())
}

#[tracing::instrument(level = "debug", skip_all)]
/// The `cache clear --dry-run` report: what a real clear would delete.
fn cache_clear_dry_run(plan: Option<&discovery::ClearPlan>) -> serde_json::Value {
    let paths = |v: &[std::path::PathBuf]| -> Vec<String> {
        v.iter().map(|p| p.display().to_string()).collect()
    };
    match plan {
        Some(plan) => serde_json::json!({
            "dry_run": true,
            "wouldRemove": plan.documents.len(),
            "directory": plan.dir.as_ref().map(|d| d.display().to_string()),
            "documents": paths(&plan.documents),
            "tempFiles": paths(&plan.temp_files),
        }),
        None => serde_json::json!({
            "dry_run": true,
            "wouldRemove": 0,
            "directory": null,
            "documents": [],
            "tempFiles": [],
        }),
    }
}

async fn dispatch(
    cli: Cli,
    args: &[OsString],
    settings: &Settings,
    prescan: &PreScan,
) -> Result<(), CliError> {
    let Some(command) = cli.command else {
        // `arg_required_else_help` makes this unreachable from the CLI.
        return Err(GwsError::Validation("no command given; run `gwsr --help`".into()).into());
    };
    if let TopCommand::Service(service_args) = command {
        return run_service(args, &service_args, settings, prescan).await;
    }

    install_output(&cli.global, settings)?;
    let format = effective_format(&cli.global, settings)?;
    tracing::debug!(?command, ?format, "dispatching static command");
    // `auth` also accepts --profile/--impersonate after its own name and
    // records the overrides itself.
    if !matches!(command, TopCommand::Auth { .. }) {
        auth::profiles::set_global_overrides(cli.global.auth_overrides())?;
    }
    match command {
        TopCommand::Auth { args } => {
            auth::commands::handle_auth_command(
                &args,
                cli.global.auth_overrides(),
                cli.global.dry_run,
            )
            .await?
        }
        TopCommand::Batch(batch_args) => {
            let ctx = executor::batch::BatchContext {
                dry_run: cli.global.dry_run,
                api_version: cli.global.api_version.clone(),
                sanitize: sanitize_config(&cli.global, settings)?,
            };
            executor::batch::handle_batch_command(&batch_args, &ctx).await?;
        }
        TopCommand::Schema { path, resolve_refs } => {
            let value = schema::handle_schema_command(&path, resolve_refs).await?;
            emit_value(&value, format)?;
        }
        TopCommand::Commands { services } => {
            let inventory = inventory::build(&services).await?;
            match format {
                // Tabular formats get one row per command.
                OutputFormat::Table | OutputFormat::Csv => {
                    emit_value(&inventory::rows(&inventory), format)?;
                }
                OutputFormat::Json | OutputFormat::Yaml => emit_value(&inventory, format)?,
            }
        }
        TopCommand::Completions { shell } => {
            let script = completions::registration_script(shell, "gwsr")?;
            output::emit(script.trim_end())?;
        }
        TopCommand::Cache {
            action: CacheCommand::Clear,
        } => {
            let loader = discovery::loader()?;
            if cli.global.dry_run {
                let plan = loader.clear_cache_plan().map_err(GwsError::from)?;
                emit_value(&cache_clear_dry_run(plan.as_ref()), format)?;
            } else {
                let removed = loader.clear_cache().map_err(GwsError::from)?;
                emit_value(&serde_json::json!({ "removed": removed }), format)?;
            }
        }
        TopCommand::Dev {
            command: DevCommand::GenerateSkills(opts),
        } => {
            let summary =
                generate_skills::handle_generate_skills(&opts, cli.global.dry_run).await?;
            emit_value(&summary, format)?;
        }
        TopCommand::Dev {
            command: DevCommand::Man { output_dir },
        } => {
            let dry_run = cli.global.dry_run;
            let files = completions::write_man_pages(&output_dir, dry_run)?;
            let names: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            let summary = if dry_run {
                serde_json::json!({ "dry_run": true, "wouldWrite": names })
            } else {
                serde_json::json!({ "written": names })
            };
            emit_value(&summary, format)?;
        }
        TopCommand::Help { .. } | TopCommand::Service(_) => {
            // Handled by `parse_top_level` / above.
        }
    }
    Ok(())
}

/// Split `service[:version]`, resolve the alias (or any `<api>:<version>`),
/// and apply `--api-version` (which wins over the `:version` suffix).
/// Returns `(alias, api_name, version)`.
pub fn parse_service_and_version(
    service_token: &str,
    api_version_flag: Option<&str>,
) -> Result<(String, String, String), GwsError> {
    let alias = service_token
        .split_once(':')
        .map_or(service_token, |(name, _)| name);
    let resolved = services::resolve_service_spec(service_token, api_version_flag)?;
    Ok((alias.to_string(), resolved.api_name, resolved.version))
}

/// argv for the dynamic service command: the top-level global flags that
/// preceded the service name, then everything after it.
fn service_argv(args: &[OsString], service_args: &[OsString], alias: &str) -> Vec<OsString> {
    let service_index = args.len().saturating_sub(service_args.len());
    let mut argv = vec![OsString::from(format!("gwsr {alias}"))];
    argv.extend(args.iter().take(service_index).skip(1).cloned());
    argv.extend(service_args.iter().skip(1).cloned());
    argv
}

#[tracing::instrument(level = "debug", skip_all)]
async fn run_service(
    args: &[OsString],
    service_args: &[OsString],
    settings: &Settings,
    prescan: &PreScan,
) -> Result<(), CliError> {
    let service_token = service_args
        .first()
        .and_then(|s| s.to_str())
        .ok_or_else(|| GwsError::Validation("the service name must be valid UTF-8".into()))?;
    let (alias, api_name, version) =
        parse_service_and_version(service_token, prescan.api_version.as_deref())?;
    tracing::debug!(service = %alias, api = %api_name, version = %version, "resolving service");

    let doc = service::load_document(&api_name, &version).await?;
    // Usage lines show the service as typed, so `youtube:v3` stays runnable.
    let cmd = service::build_command(service_token, &doc);
    let matches = match cmd.try_get_matches_from(service_argv(args, service_args, service_token)) {
        Ok(m) => m,
        Err(e) if is_display_request(&e) => return print_display(&e),
        Err(e) => return Err(e.into()),
    };

    let global = GlobalArgs::from_arg_matches(&matches)?;
    install_output(&global, settings)?;
    auth::profiles::set_global_overrides(global.auth_overrides())?;
    error::set_context(ErrorContext {
        service: Some(alias.clone()),
        ..Default::default()
    });

    let output_format = OutputFormat::from_matches(&matches)?;
    let sanitize_config = sanitize_config(&global, settings)?;

    // Check if a helper wants to handle this command
    if let Some(helper) = helpers::get_helper(&doc.name)
        && helper.handle(&doc, &matches, &sanitize_config).await?
    {
        return Ok(());
    }

    // Walk the subcommand tree to find the target method
    let (method, matched_args) = resolve_method_from_matches(&doc, &matches)?;
    tracing::info!(
        method = method.id.as_deref().unwrap_or("?"),
        http_method = %method.http_method,
        "resolved API method"
    );
    error::set_context(ErrorContext {
        service: Some(alias.clone()),
        required_scopes: method.scopes.clone(),
        granted_scopes: None,
    });

    executor::options::run_from_matches(
        &doc,
        method,
        matched_args,
        &sanitize_config,
        &output_format,
        &executor::options::ConfigDefaults {
            page_limit: settings.page_limit,
            page_delay_ms: settings.page_delay_ms,
        },
    )
    .await?;
    Ok(())
}

/// Recursively walks clap ArgMatches to find the leaf method and its matches.
fn resolve_method_from_matches<'a>(
    doc: &'a discovery::RestDescription,
    matches: &'a clap::ArgMatches,
) -> Result<(&'a discovery::RestMethod, &'a clap::ArgMatches), GwsError> {
    // Walk the subcommand chain
    let mut path: Vec<&str> = Vec::new();
    let mut current_matches = matches;

    while let Some((sub_name, sub_matches)) = current_matches.subcommand() {
        path.push(sub_name);
        current_matches = sub_matches;
    }

    if path.is_empty() {
        return Err(GwsError::Validation(
            "No resource or method specified".to_string(),
        ));
    }

    // A method declared outside any resource: ["tokeninfo"]
    if let [method_name] = path.as_slice()
        && let Some(method) = doc.methods.get(*method_name)
    {
        return Ok((method, current_matches));
    }

    // path looks like ["files", "list"] or ["files", "permissions", "list"]
    // Walk the Discovery Document resources to find the method
    let resource_name = path[0];
    let resource = doc
        .resources
        .get(resource_name)
        .ok_or_else(|| GwsError::Validation(format!("Resource '{resource_name}' not found")))?;

    let mut current_resource = resource;

    // Navigate sub-resources (everything except the last element, which is the method)
    for &name in &path[1..path.len() - 1] {
        // Check if this is a sub-resource
        if let Some(sub) = current_resource.resources.get(name) {
            current_resource = sub;
        } else {
            return Err(GwsError::Validation(format!(
                "Sub-resource '{name}' not found"
            )));
        }
    }

    // The last element is the method name
    let method_name = path[path.len() - 1];

    // Check if this is a method on the current resource
    if let Some(method) = current_resource.methods.get(method_name) {
        return Ok((method, current_matches));
    }

    // Maybe it's a resource that has methods — need one more subcommand
    Err(GwsError::Validation(format!(
        "Method '{method_name}' not found on resource. Available methods: {:?}",
        current_resource.methods.keys().collect::<Vec<_>>()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_method_from_matches_basic() {
        let mut resources = std::collections::HashMap::new();
        let mut files_res = crate::discovery::RestResource::default();
        files_res.methods.insert(
            "list".to_string(),
            crate::discovery::RestMethod {
                id: Some("drive.files.list".to_string()),
                http_method: "GET".to_string(),
                ..Default::default()
            },
        );
        resources.insert("files".to_string(), files_res);

        let doc = discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };

        // Simulate CLI structure
        let cmd = clap::Command::new("gwsr")
            .subcommand(clap::Command::new("files").subcommand(clap::Command::new("list")));

        let matches = cmd.get_matches_from(vec!["gwsr", "files", "list"]);
        let (method, _) = resolve_method_from_matches(&doc, &matches).unwrap();
        assert_eq!(method.id.as_deref(), Some("drive.files.list"));
    }

    #[test]
    fn test_resolve_method_from_matches_nested() {
        let mut resources = std::collections::HashMap::new();
        let mut files_res = crate::discovery::RestResource::default();
        let mut permissions_res = crate::discovery::RestResource::default();
        permissions_res.methods.insert(
            "get".to_string(),
            crate::discovery::RestMethod {
                id: Some("drive.files.permissions.get".to_string()),
                ..Default::default()
            },
        );
        files_res
            .resources
            .insert("permissions".to_string(), permissions_res);
        resources.insert("files".to_string(), files_res);

        let doc = discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };

        let cmd =
            clap::Command::new("gwsr").subcommand(clap::Command::new("files").subcommand(
                clap::Command::new("permissions").subcommand(clap::Command::new("get")),
            ));

        let matches = cmd.get_matches_from(vec!["gwsr", "files", "permissions", "get"]);
        let (method, _) = resolve_method_from_matches(&doc, &matches).unwrap();
        assert_eq!(method.id.as_deref(), Some("drive.files.permissions.get"));
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn service_argv_moves_leading_globals_after_the_service() {
        let args = os(&["gwsr", "--format", "table", "drive", "files", "list"]);
        let service_args = os(&["drive", "files", "list"]);
        assert_eq!(
            service_argv(&args, &service_args, "drive"),
            os(&["gwsr drive", "--format", "table", "files", "list"])
        );
        // A flag value equal to the service name is not mistaken for it.
        let args = os(&["gwsr", "--jq", "drive", "drive", "files"]);
        let service_args = os(&["drive", "files"]);
        assert_eq!(
            service_argv(&args, &service_args, "drive"),
            os(&["gwsr drive", "--jq", "drive", "files"])
        );
    }

    #[test]
    fn parse_service_and_version_variants() {
        assert_eq!(
            parse_service_and_version("drive", None).unwrap(),
            ("drive".into(), "drive".into(), "v3".into())
        );
        assert_eq!(parse_service_and_version("drive:v2", None).unwrap().2, "v2");
        // --api-version wins over the suffix.
        assert_eq!(
            parse_service_and_version("drive:v2", Some("v1")).unwrap().2,
            "v1"
        );
        assert!(parse_service_and_version("nope", None).is_err());
    }

    #[test]
    fn broken_pipe_panic_detection() {
        assert!(is_stdout_broken_pipe_panic(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(is_stdout_broken_pipe_panic(
            "failed printing to stdout: The pipe is being closed. (os error 232)"
        ));
        assert!(!is_stdout_broken_pipe_panic(
            "failed printing to stdout: No space left on device (os error 28)"
        ));
        assert!(!is_stdout_broken_pipe_panic("index out of bounds"));
    }

    #[test]
    fn help_rewrites_to_help_flag() {
        // `gwsr help schema` prints schema help and yields no command.
        assert!(
            parse_top_level(&os(&["gwsr", "help", "schema"]))
                .unwrap()
                .is_none()
        );
        // `gwsr help drive files` becomes `gwsr drive files --help`.
        let cli = parse_top_level(&os(&["gwsr", "help", "drive", "files"]))
            .unwrap()
            .unwrap();
        match cli.command {
            Some(TopCommand::Service(args)) => {
                assert_eq!(args, os(&["drive", "files", "--help"]));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
