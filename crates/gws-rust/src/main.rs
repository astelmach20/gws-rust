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

mod auth;
pub(crate) mod auth_commands;
mod cache;
mod cli_args;
mod client;
mod commands;
mod completions;
mod config;
pub(crate) mod credential_store;
mod discovery;
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
mod oauth_config;
mod output;
mod schema;
mod service;
mod services;
mod setup;
mod setup_tui;
mod text;
mod timezone;
mod token_storage;
pub(crate) mod validate;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{FromArgMatches, Parser};

use cli_args::{CacheCommand, Cli, DevCommand, GlobalArgs, PreScan, TopCommand};
use config::{JsonStylePref, SanitizeModePref, Settings};
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
    // Before the config file is read, the error format comes from the flag or env.
    let early_human = human_output(prescan.format.as_deref(), None);

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

    let mut log_opts = logging::LogOptions::from_env(prescan.verbosity);
    log_opts.config_log = settings.log.clone();
    log_opts.human = human || prescan.verbosity > 0;
    if log_opts.file_dir.is_none() {
        log_opts.file_dir = settings.log_file.clone();
    }
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

    tracing::debug!(config = %config::config_path().display(), ?settings, "resolved settings");
    let result = runtime.block_on(dispatch(cli, &args, &settings, &prescan));
    // Stop background tasks, then flush file logs before exiting.
    drop(runtime);
    let code = finish(result, human);
    drop(log_guard);
    code
}

/// True when the effective output format is a human one (not JSON):
/// `--format` flag > `GWSR_FORMAT` > config file > JSON.
fn human_output(flag: Option<&str>, config: Option<OutputFormat>) -> bool {
    let from_flag = flag.and_then(|f| OutputFormat::parse(f).ok());
    let from_env = std::env::var("GWSR_FORMAT")
        .ok()
        .and_then(|f| OutputFormat::parse(&f).ok());
    let format = from_flag.or(from_env).or(config).unwrap_or_default();
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
        default_format: settings.format,
        columns: global.columns.clone(),
        jq,
        terminal,
    })?;
    Ok(())
}

/// The effective format for static commands (flag > env/config).
fn effective_format(global: &GlobalArgs, settings: &Settings) -> OutputFormat {
    global
        .format
        .as_deref()
        .map(OutputFormat::from_str)
        .unwrap_or(settings.format)
}

fn emit_value(value: &serde_json::Value, format: OutputFormat) -> Result<(), CliError> {
    let text = formatter::format_value(value, &format)?;
    output::emit(&text)?;
    Ok(())
}

#[tracing::instrument(level = "debug", skip_all)]
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
    let format = effective_format(&cli.global, settings);
    tracing::debug!(?command, ?format, "dispatching static command");
    match command {
        TopCommand::Auth { args } => auth_commands::handle_auth_command(&args).await?,
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
            let removed = cache::clear_cache()?;
            emit_value(&serde_json::json!({ "removed": removed }), format)?;
        }
        TopCommand::Dev {
            command: DevCommand::GenerateSkills(opts),
        } => {
            let summary = generate_skills::handle_generate_skills(&opts).await?;
            emit_value(&summary, format)?;
        }
        TopCommand::Dev {
            command: DevCommand::Man { output_dir },
        } => {
            let files = completions::write_man_pages(&output_dir)?;
            let names: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            emit_value(&serde_json::json!({ "written": names }), format)?;
        }
        TopCommand::Help { .. } | TopCommand::Service(_) => {
            // Handled by `parse_top_level` / above.
        }
    }
    Ok(())
}

/// Split `service[:version]`, resolve the alias, and apply `--api-version`
/// (which wins over the `:version` suffix).
pub fn parse_service_and_version(
    service_token: &str,
    api_version_flag: Option<&str>,
) -> Result<(String, String, String), GwsError> {
    let (alias, suffix_version) = match service_token.split_once(':') {
        Some((svc, ver)) => (svc, Some(ver)),
        None => (service_token, None),
    };
    let (api_name, default_version) = services::resolve_service(alias)?;
    let version = api_version_flag
        .or(suffix_version)
        .map_or(default_version, str::to_string);
    Ok((alias.to_string(), api_name, version))
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
    let cmd = service::build_command(&alias, &doc);
    let matches = match cmd.try_get_matches_from(service_argv(args, service_args, &alias)) {
        Ok(m) => m,
        Err(e) if is_display_request(&e) => return print_display(&e),
        Err(e) => return Err(e.into()),
    };

    let global = GlobalArgs::from_arg_matches(&matches)?;
    install_output(&global, settings)?;
    error::set_context(ErrorContext {
        service: Some(alias.clone()),
        ..Default::default()
    });

    let output_format = OutputFormat::from_matches(&matches);
    let sanitize_config = helpers::modelarmor::SanitizeConfig {
        template: global
            .sanitize
            .clone()
            .or_else(|| settings.sanitize_template.clone()),
        // The mode string is already validated by `config::resolve`.
        mode: helpers::modelarmor::SanitizeMode::from_str(match settings.sanitize_mode {
            SanitizeModePref::Warn => "warn",
            SanitizeModePref::Block => "block",
        }),
    };

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

    let params_json = matched_args.get_one::<String>("params").map(|s| s.as_str());
    let body_json = matched_args
        .try_get_one::<String>("json")
        .ok()
        .flatten()
        .map(|s| s.as_str());
    let upload_path = matched_args
        .try_get_one::<String>("upload")
        .ok()
        .flatten()
        .map(|s| s.as_str());
    let output_path = matched_args.get_one::<String>("output").map(|s| s.as_str());

    // Validate file paths against traversal before any I/O.
    // Use the returned canonical paths so the validated path is the one
    // actually used for I/O (closes TOCTOU gap).
    let upload_path_buf = if let Some(p) = upload_path {
        Some(crate::validate::validate_safe_file_path(p, "--upload")?)
    } else {
        None
    };
    let output_path_buf = if let Some(p) = output_path {
        Some(crate::validate::validate_safe_file_path(p, "--output")?)
    } else {
        None
    };
    let upload_path = upload_path_buf.as_deref().and_then(|p| p.to_str());
    let output_path = output_path_buf.as_deref().and_then(|p| p.to_str());

    let upload = {
        let upload_content_type = matched_args
            .try_get_one::<String>("upload-content-type")
            .ok()
            .flatten()
            .map(|s| s.as_str());
        upload_path.map(|path| executor::UploadSource::File {
            path,
            content_type: upload_content_type,
        })
    };

    let dry_run = matched_args.get_flag("dry-run");

    // Build pagination config from flags; config/env supply the defaults.
    let mut pagination = parse_pagination_config(matched_args);
    if matched_args.get_one::<u32>("page-limit").is_none()
        && let Some(limit) = settings.page_limit
    {
        pagination.page_limit = limit;
    }
    if matched_args.get_one::<u64>("page-delay").is_none()
        && let Some(delay) = settings.page_delay_ms
    {
        pagination.page_delay_ms = delay;
    }

    // Select the best scope for the method. Discovery Documents list scopes as
    // alternatives (any one grants access). We pick the first (broadest) scope
    // to avoid restrictive scopes like gmail.metadata that block query parameters.
    let scopes: Vec<&str> = select_scope(&method.scopes).into_iter().collect();

    // Authenticate: try OAuth, fail with error if credentials exist but are broken
    tracing::debug!(?scopes, "requesting access token");
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(e) => {
            // If credentials were found but failed (e.g. decryption error, invalid token),
            // propagate the error instead of silently falling back to unauthenticated.
            // Only fall back to None if no credentials exist at all.
            let err_msg = format!("{e:#}");
            // NB: matches the bail!() message in auth::load_credentials_inner
            if err_msg.starts_with("No credentials found") {
                tracing::info!("no credentials configured; sending the request unauthenticated");
                (None, executor::AuthMethod::None)
            } else {
                return Err(GwsError::Auth(format!("Authentication failed: {err_msg}")).into());
            }
        }
    };

    // Execute
    executor::execute_method(
        &doc,
        method,
        params_json,
        body_json,
        token.as_deref(),
        auth_method,
        output_path,
        upload,
        dry_run,
        &pagination,
        sanitize_config.template.as_deref(),
        &sanitize_config.mode,
        &output_format,
        false,
    )
    .await?;
    Ok(())
}

/// Select the best scope from a method's scope list.
///
/// Discovery Documents list method scopes as alternatives — any single scope
/// grants access. The first scope is typically the broadest. Using all scopes
/// causes issues when restrictive scopes (e.g., `gmail.metadata`) are included,
/// as the API enforces that scope's restrictions even when broader scopes are
/// also present.
pub(crate) fn select_scope(scopes: &[String]) -> Option<&str> {
    scopes.first().map(|s| s.as_str())
}

fn parse_pagination_config(matches: &clap::ArgMatches) -> executor::PaginationConfig {
    executor::PaginationConfig {
        page_all: matches.get_flag("page-all"),
        page_limit: matches.get_one::<u32>("page-limit").copied().unwrap_or(10),
        page_delay_ms: matches.get_one::<u64>("page-delay").copied().unwrap_or(100),
    }
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
    fn test_parse_pagination_config_defaults() {
        let matches = clap::Command::new("test")
            .arg(
                clap::Arg::new("page-all")
                    .long("page-all")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("page-limit")
                    .long("page-limit")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                clap::Arg::new("page-delay")
                    .long("page-delay")
                    .value_parser(clap::value_parser!(u64)),
            )
            .get_matches_from(vec!["test"]);

        let config = parse_pagination_config(&matches);
        assert!(!config.page_all);
        assert_eq!(config.page_limit, 10);
        assert_eq!(config.page_delay_ms, 100);
    }

    #[test]
    fn test_parse_pagination_config_custom() {
        let matches = clap::Command::new("test")
            .arg(
                clap::Arg::new("page-all")
                    .long("page-all")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("page-limit")
                    .long("page-limit")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                clap::Arg::new("page-delay")
                    .long("page-delay")
                    .value_parser(clap::value_parser!(u64)),
            )
            .get_matches_from(vec![
                "test",
                "--page-all",
                "--page-limit",
                "20",
                "--page-delay",
                "500",
            ]);

        let config = parse_pagination_config(&matches);
        assert!(config.page_all);
        assert_eq!(config.page_limit, 20);
        assert_eq!(config.page_delay_ms, 500);
    }

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

    #[test]
    fn test_select_scope_picks_first() {
        let scopes = vec![
            "https://mail.google.com/".to_string(),
            "https://www.googleapis.com/auth/gmail.metadata".to_string(),
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        ];
        assert_eq!(select_scope(&scopes), Some("https://mail.google.com/"));
    }

    #[test]
    fn test_select_scope_single() {
        let scopes = vec!["https://www.googleapis.com/auth/drive".to_string()];
        assert_eq!(
            select_scope(&scopes),
            Some("https://www.googleapis.com/auth/drive")
        );
    }

    #[test]
    fn test_select_scope_empty() {
        let scopes: Vec<String> = vec![];
        assert_eq!(select_scope(&scopes), None);
    }
}
