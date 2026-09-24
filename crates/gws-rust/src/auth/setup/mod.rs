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

//! `gwsr auth setup`: GCP project and OAuth client bootstrap.
//!
//! Uses gcloud for account/project selection and API enabling, the OAuth2
//! REST API for the consent screen, and asks the user to paste the Desktop
//! client ID/secret (Google no longer allows creating OAuth clients by API).

pub mod apis;
pub mod gcloud;
pub mod messages;
pub mod tui;

use std::io::IsTerminal;
use std::path::PathBuf;

use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

use crate::error::GwsError;
use gcloud::{EnableResult, Gcloud};
use tui::{InputResult, PickerResult, SelectItem, SetupWizard, StepStatus};

/// Options for the setup command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupOptions {
    pub project: Option<String>,
    pub dry_run: bool,
    pub login: bool,
    /// Never start the terminal UI or prompt, even on a terminal.
    pub non_interactive: bool,
}

fn setup_command() -> clap::Command {
    clap::Command::new("setup")
        .bin_name("gwsr auth setup")
        .about("Create a GCP project + OAuth client with gcloud (interactive)")
        .arg(
            clap::Arg::new("project")
                .long("project")
                .help("Use a specific GCP project")
                .value_name("ID"),
        )
        .arg(
            clap::Arg::new("login")
                .long("login")
                .help("Run `gwsr auth login` after successful setup")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("dry-run")
                .long("dry-run")
                .help("Preview changes without making them")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("non-interactive")
                .long("non-interactive")
                .help("Never show the terminal UI or prompt; print JSON with the next steps")
                .action(clap::ArgAction::SetTrue),
        )
}

/// Parse setup flags; `Ok(None)` if help was printed.
///
/// # Errors
///
/// Invalid arguments.
pub fn parse_setup_args(args: &[String]) -> Result<Option<SetupOptions>, GwsError> {
    let Some(m) = crate::auth::commands::parse_or_help(setup_command(), "setup", args)? else {
        return Ok(None);
    };
    Ok(Some(SetupOptions {
        project: crate::args::value::<String>(&m, "project")?.cloned(),
        dry_run: crate::args::flag(&m, "dry-run")?,
        login: crate::args::flag(&m, "login")?,
        non_interactive: crate::args::flag(&m, "non-interactive")?,
    }))
}

const STEP_LABELS: [&str; 5] = [
    "gcloud CLI",
    "Authentication",
    "GCP project",
    "Workspace APIs",
    "OAuth credentials",
];

const LOGIN_NEW_ACCOUNT: &str = "+ Log in with a new account";
const CREATE_PROJECT: &str = "+ Create a new project";
const ENTER_PROJECT: &str = "> Enter a project ID";

enum Stage {
    CheckGcloud,
    Account,
    Project,
    EnableApis,
    ConfigureOauth,
    Finish,
}

/// What the user chose in the account picker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AccountChoice {
    LoginNew,
    Existing(String),
    None,
}

/// What the user chose in the project picker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProjectChoice {
    Create,
    Enter,
    Existing(String),
    None,
}

pub(crate) fn account_choice(items: &[SelectItem]) -> AccountChoice {
    match items.iter().find(|i| i.selected) {
        Some(i) if i.label == LOGIN_NEW_ACCOUNT => AccountChoice::LoginNew,
        Some(i) => AccountChoice::Existing(i.label.clone()),
        None => AccountChoice::None,
    }
}

pub(crate) fn project_choice(items: &[SelectItem]) -> ProjectChoice {
    match items.iter().find(|i| i.selected) {
        Some(i) if i.label == CREATE_PROJECT => ProjectChoice::Create,
        Some(i) if i.label == ENTER_PROJECT => ProjectChoice::Enter,
        Some(i) => ProjectChoice::Existing(i.label.clone()),
        None => ProjectChoice::None,
    }
}

pub(crate) fn api_choice(items: &[SelectItem]) -> Vec<String> {
    items
        .iter()
        .zip(apis::WORKSPACE_APIS)
        .filter(|(item, _)| item.selected)
        .map(|(_, api)| api.id.to_string())
        .collect()
}

fn item(label: &str, description: &str, selected: bool) -> SelectItem {
    SelectItem {
        label: label.to_string(),
        description: description.to_string(),
        selected,
        is_fixed: false,
        is_template: false,
        template_selects: vec![],
    }
}

fn tui_err(e: std::io::Error) -> GwsError {
    GwsError::Validation(format!("terminal UI error: {e}"))
}

/// `auth setup` delegates sign-in, account and project selection to the
/// gcloud CLI; a failed gcloud step means setting up authentication failed,
/// so it is reported as an auth error (exit code 2) with gcloud's output.
fn gcloud_err(e: anyhow::Error) -> GwsError {
    GwsError::Auth(format!("{e:#}"))
}

struct Ctx {
    wizard: Option<SetupWizard>,
    interactive: bool,
    opts: SetupOptions,
    gcloud: Gcloud,
    client_path: PathBuf,
    account: String,
    project_id: String,
    api_ids: Vec<String>,
    enable: EnableResult,
    consent_note: Option<String>,
    /// Set when setup could not finish non-interactively.
    manual_steps: Option<String>,
}

impl Ctx {
    fn wizard(&mut self) -> Result<&mut SetupWizard, GwsError> {
        self.wizard
            .as_mut()
            .ok_or_else(|| GwsError::Validation("interactive setup requires a terminal".into()))
    }

    fn step(&mut self, idx: usize, status: StepStatus) -> Result<(), GwsError> {
        match self.wizard.as_mut() {
            Some(w) => w.update_step(idx, status).map_err(tui_err),
            None => Ok(()),
        }
    }

    fn message(&mut self, msg: &str) -> Result<(), GwsError> {
        match self.wizard.as_mut() {
            Some(w) => w.show_message(msg).map_err(tui_err),
            None => Ok(()),
        }
    }

    fn finish_wizard(&mut self) -> Result<(), GwsError> {
        match self.wizard.take() {
            Some(w) => w.finish().map_err(tui_err),
            None => Ok(()),
        }
    }

    fn cancelled(&mut self) -> GwsError {
        if let Err(e) = self.finish_wizard() {
            tracing::warn!("{e}");
        }
        GwsError::Validation("setup cancelled".into())
    }
}

async fn stage_check_gcloud(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    ctx.step(0, StepStatus::InProgress("checking...".into()))?;
    if !ctx.gcloud.is_installed().await.map_err(gcloud_err)? {
        ctx.step(0, StepStatus::Failed("not found".into()))?;
        ctx.finish_wizard()?;
        return Err(GwsError::Validation(messages::gcloud_missing_message(
            &ctx.client_path,
        )));
    }
    ctx.step(0, StepStatus::Done("found".into()))?;
    if !ctx.interactive {
        crate::output::eprint_line("Step 1/5: gcloud CLI found");
    }
    Ok(Stage::Account)
}

async fn stage_account(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    ctx.step(1, StepStatus::InProgress(String::new()))?;
    if !ctx.interactive {
        let account = match ctx.gcloud.account().await.map_err(gcloud_err)? {
            Some(a) => a,
            None if ctx.opts.dry_run => {
                return Err(GwsError::Validation(
                    "no active gcloud account; run `gcloud auth login` first".into(),
                ));
            }
            None => {
                crate::output::eprint_line(
                    "Step 2/5: not logged in to gcloud; running `gcloud auth login`...",
                );
                ctx.gcloud.auth_login().await.map_err(gcloud_err)?;
                ctx.gcloud
                    .account()
                    .await
                    .map_err(gcloud_err)?
                    .ok_or_else(|| {
                        GwsError::Auth("gcloud login finished but no account is active".into())
                    })?
            }
        };
        crate::output::eprint_line(&format!("Step 2/5: authenticated as {account}"));
        ctx.account = account;
        return Ok(Stage::Project);
    }

    let accounts = ctx.gcloud.accounts().await.map_err(gcloud_err)?;
    let current = ctx
        .gcloud
        .account()
        .await
        .map_err(gcloud_err)?
        .unwrap_or_default();
    let mut items = vec![item(
        LOGIN_NEW_ACCOUNT,
        "Opens a browser for gcloud auth login",
        false,
    )];
    items.extend(accounts.iter().map(|(acct, active)| {
        item(
            acct,
            if *active { "(active)" } else { "" },
            *acct == current,
        )
    }));
    let result = ctx
        .wizard()?
        .show_picker("Select a Google account", "Enter to confirm", items, false)
        .map_err(tui_err)?;
    let items = match result {
        PickerResult::Confirmed(items) => items,
        PickerResult::GoBack => return Ok(Stage::CheckGcloud),
        PickerResult::Cancelled => return Err(ctx.cancelled()),
    };
    let account = match account_choice(&items) {
        AccountChoice::LoginNew => {
            ctx.wizard()?.suspend().map_err(tui_err)?;
            crate::output::eprint_line("Opening a browser for `gcloud auth login`...");
            let login = ctx.gcloud.auth_login().await;
            ctx.wizard()?.resume().map_err(tui_err)?;
            login.map_err(gcloud_err)?;
            ctx.gcloud
                .account()
                .await
                .map_err(gcloud_err)?
                .ok_or_else(|| {
                    GwsError::Auth("gcloud login finished but no account is active".into())
                })?
        }
        AccountChoice::Existing(acct) => {
            ctx.gcloud.set_account(&acct).await.map_err(gcloud_err)?;
            acct
        }
        AccountChoice::None => {
            ctx.finish_wizard()?;
            return Err(GwsError::Validation("no account selected".into()));
        }
    };
    ctx.step(1, StepStatus::Done(account.clone()))?;
    ctx.account = account;
    Ok(Stage::Project)
}

async fn create_project_loop(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    let mut last: Option<String> = None;
    loop {
        let input = ctx
            .wizard()?
            .show_input(
                "Create a new GCP project",
                "Enter a unique project ID",
                last.as_deref(),
            )
            .map_err(tui_err)?;
        let project = match input {
            InputResult::Confirmed(v) if v.trim().is_empty() => {
                ctx.message("Project ID cannot be empty. Enter an ID, press Up to go back, or Esc to cancel.")?;
                continue;
            }
            InputResult::Confirmed(v) => v.trim().to_string(),
            InputResult::GoBack => return Ok(Stage::Project),
            InputResult::Cancelled => return Err(ctx.cancelled()),
        };
        ctx.message(&format!("Creating project '{project}'..."))?;
        match ctx.gcloud.create_project(&project).await {
            Ok(()) => {
                ctx.gcloud.set_project(&project).await.map_err(gcloud_err)?;
                ctx.step(2, StepStatus::Done(project.clone()))?;
                ctx.project_id = project;
                return Ok(Stage::EnableApis);
            }
            Err(out) => {
                let msg = messages::format_project_create_failure(&project, &ctx.account, &out);
                ctx.message(&format!(
                    "{msg}\n\nTry another project ID, press Up to go back, or Esc to cancel."
                ))?;
                last = Some(project);
            }
        }
    }
}

async fn stage_project(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    ctx.step(2, StepStatus::InProgress(String::new()))?;
    if let Some(p) = ctx.opts.project.clone() {
        if !ctx.opts.dry_run {
            ctx.gcloud.set_project(&p).await.map_err(gcloud_err)?;
        }
        ctx.step(2, StepStatus::Done(p.clone()))?;
        if !ctx.interactive {
            crate::output::eprint_line(&format!("Step 3/5: project {p}"));
        }
        ctx.project_id = p;
        return Ok(Stage::EnableApis);
    }
    if !ctx.interactive {
        let p = ctx.gcloud.project().await.map_err(gcloud_err)?.ok_or_else(|| {
            GwsError::Validation(
                "no GCP project configured; pass --project <id> or run `gcloud config set project <id>`"
                    .into(),
            )
        })?;
        crate::output::eprint_line(&format!("Step 3/5: using the current project {p}"));
        ctx.project_id = p;
        return Ok(Stage::EnableApis);
    }

    ctx.message("Loading projects...")?;
    let projects = match ctx.gcloud.projects().await {
        Ok(p) => p,
        Err(e) => {
            ctx.message(&format!("Could not list projects: {e:#}"))?;
            Vec::new()
        }
    };
    let current = ctx
        .gcloud
        .project()
        .await
        .map_err(gcloud_err)?
        .unwrap_or_default();
    let mut items = vec![
        item(CREATE_PROJECT, "Create a GCP project for gwsr", false),
        item(ENTER_PROJECT, "Use an existing project ID you know", false),
    ];
    items.extend(
        projects
            .iter()
            .map(|(id, name)| item(id, name, *id == current)),
    );
    let result = ctx
        .wizard()?
        .show_picker("Select a GCP project", "Enter to confirm", items, false)
        .map_err(tui_err)?;
    let items = match result {
        PickerResult::Confirmed(items) => items,
        PickerResult::GoBack => return Ok(Stage::Account),
        PickerResult::Cancelled => return Err(ctx.cancelled()),
    };
    let project = match project_choice(&items) {
        ProjectChoice::Create => return create_project_loop(ctx).await,
        ProjectChoice::Enter => {
            match ctx
                .wizard()?
                .show_input(
                    "Enter GCP project ID",
                    "Type your existing project ID",
                    None,
                )
                .map_err(tui_err)?
            {
                InputResult::Confirmed(v) if !v.trim().is_empty() => v.trim().to_string(),
                InputResult::Confirmed(_) | InputResult::GoBack => return Ok(Stage::Project),
                InputResult::Cancelled => return Err(ctx.cancelled()),
            }
        }
        ProjectChoice::Existing(p) => p,
        ProjectChoice::None => {
            ctx.finish_wizard()?;
            return Err(GwsError::Validation(
                "no project selected; use --project <id>".into(),
            ));
        }
    };
    ctx.gcloud.set_project(&project).await.map_err(gcloud_err)?;
    ctx.step(2, StepStatus::Done(project.clone()))?;
    ctx.project_id = project;
    Ok(Stage::EnableApis)
}

async fn stage_enable_apis(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    ctx.step(3, StepStatus::InProgress(String::new()))?;
    if ctx.interactive {
        let already = ctx
            .gcloud
            .enabled_apis(&ctx.project_id)
            .await
            .map_err(gcloud_err)?;
        let items: Vec<SelectItem> = apis::WORKSPACE_APIS
            .iter()
            .map(|api| {
                let on = already.iter().any(|a| a == api.id);
                SelectItem {
                    label: api.name.to_string(),
                    description: if on {
                        format!("{} (already enabled)", api.id)
                    } else {
                        api.id.to_string()
                    },
                    selected: on,
                    is_fixed: on,
                    is_template: false,
                    template_selects: vec![],
                }
            })
            .collect();
        let result = ctx
            .wizard()?
            .show_picker(
                "Select APIs to enable",
                "Space to toggle, 'a' to select all, Enter to confirm",
                items,
                true,
            )
            .map_err(tui_err)?;
        match result {
            PickerResult::Confirmed(items) => ctx.api_ids = api_choice(&items),
            PickerResult::GoBack => return Ok(Stage::Project),
            PickerResult::Cancelled => return Err(ctx.cancelled()),
        }
    } else {
        ctx.api_ids = apis::WORKSPACE_APIS
            .iter()
            .map(|a| a.id.to_string())
            .collect();
    }

    if ctx.opts.dry_run {
        crate::output::eprint_line(&format!(
            "Step 4/5: would enable {} APIs:",
            ctx.api_ids.len()
        ));
        for id in &ctx.api_ids {
            crate::output::eprint_line(&format!("  - {id}"));
        }
        crate::output::eprint_line("Step 5/5: would configure the OAuth consent screen and client");
        return Ok(Stage::Finish);
    }

    ctx.step(
        3,
        StepStatus::InProgress(format!("enabling {} APIs...", ctx.api_ids.len())),
    )?;
    ctx.enable = ctx
        .gcloud
        .enable_apis(&ctx.project_id, &ctx.api_ids)
        .await
        .map_err(gcloud_err)?;
    let e = &ctx.enable;
    let summary = format!(
        "{} enabled, {} already enabled, {} failed",
        e.enabled.len(),
        e.skipped.len(),
        e.failed.len()
    );
    if !ctx.enable.failed.is_empty() {
        let details = ctx
            .enable
            .failed
            .iter()
            .map(|(api, err)| format!("{api}: {}", crate::output::sanitize_for_terminal(err)))
            .collect::<Vec<_>>()
            .join("\n");
        ctx.message(&format!("Some APIs could not be enabled:\n{details}"))?;
        if !ctx.interactive {
            tracing::warn!("some APIs could not be enabled:\n{details}");
        }
    }
    ctx.step(3, StepStatus::Done(summary))?;
    Ok(Stage::ConfigureOauth)
}

/// Outcome of the consent-screen step.
#[derive(Debug, PartialEq, Eq)]
enum Consent {
    Exists,
    Created,
    Manual(String),
}

async fn configure_consent_screen(
    project_id: &str,
    token: &SecretString,
    support_email: &str,
) -> Result<Consent, GwsError> {
    let client = crate::auth::http::client()
        .map_err(|e| GwsError::other(format!("cannot build the HTTP client: {e:#}")))?;
    let url = format!("https://oauth2.googleapis.com/v1/projects/{project_id}/brands");
    let check = client
        .get(&url)
        .bearer_auth(token.expose_secret())
        .send()
        .await
        .map_err(|e| {
            GwsError::Network(format!("cannot check the OAuth consent screen: {e}").into())
        })?;
    if check.status().is_success() {
        let body: serde_json::Value = check
            .json()
            .await
            .map_err(|e| GwsError::other(format!("invalid consent-screen response: {e}")))?;
        if body
            .get("brands")
            .and_then(|b| b.as_array())
            .is_some_and(|b| !b.is_empty())
        {
            return Ok(Consent::Exists);
        }
    }
    let create = client
        .post(&url)
        .bearer_auth(token.expose_secret())
        .json(&json!({"applicationTitle": "gwsr CLI", "supportEmail": support_email}))
        .send()
        .await
        .map_err(|e| {
            GwsError::Network(format!("cannot create the OAuth consent screen: {e}").into())
        })?;
    let status = create.status();
    if status.is_success() {
        return Ok(Consent::Created);
    }
    let body = create.text().await.map_err(|e| {
        GwsError::Network(format!("cannot read the consent-screen response: {e}").into())
    })?;
    if body.contains("ALREADY_EXISTS") || body.contains("already exists") {
        return Ok(Consent::Exists);
    }
    Ok(Consent::Manual(format!(
        "automatic consent-screen setup failed (HTTP {status}); configure it manually at \
         https://console.cloud.google.com/apis/credentials/consent?project={project_id}"
    )))
}

async fn stage_configure_oauth(ctx: &mut Ctx) -> Result<Stage, GwsError> {
    ctx.step(4, StepStatus::InProgress("configuring...".into()))?;
    let token = ctx.gcloud.access_token().await.map_err(gcloud_err)?;
    match configure_consent_screen(&ctx.project_id, &token, &ctx.account).await? {
        Consent::Exists | Consent::Created => {}
        Consent::Manual(note) => {
            if !ctx.interactive {
                tracing::warn!("{note}");
            }
            ctx.consent_note = Some(note);
        }
    }
    if !ctx.interactive {
        let configured = crate::auth::client_config::load_from(&ctx.client_path)
            .map_err(crate::auth::to_gws_error)?
            .is_some();
        if !configured {
            ctx.manual_steps = Some(messages::manual_oauth_instructions(
                &ctx.project_id,
                &ctx.client_path,
            ));
        }
        return Ok(Stage::Finish);
    }

    let existing_id = crate::auth::client_config::load_from(&ctx.client_path)
        .map_err(crate::auth::to_gws_error)?
        .map(|c| c.client_id);
    let project = ctx.project_id.clone();
    let note = ctx
        .consent_note
        .as_ref()
        .map(|n| format!("Note: {n}\n\n"))
        .unwrap_or_default();
    ctx.message(&format!(
        "{note}Create an OAuth client (Google does not allow this by API):\n\n\
         1. Consent screen (if not configured): \
         https://console.cloud.google.com/apis/credentials/consent?project={project}\n\
         2. https://console.cloud.google.com/apis/credentials?project={project} -> Create \
         Credentials -> OAuth client ID -> Desktop app\n\n\
         Paste the Client ID and Client Secret below."
    ))?;

    let client_id = match ctx
        .wizard()?
        .show_input(
            "OAuth Client ID",
            "Paste the Client ID",
            existing_id.as_deref(),
        )
        .map_err(tui_err)?
    {
        InputResult::Confirmed(v) if !v.trim().is_empty() => v.trim().to_string(),
        InputResult::Confirmed(_) => {
            ctx.finish_wizard()?;
            return Err(GwsError::Validation("the client ID cannot be empty".into()));
        }
        InputResult::GoBack => return Ok(Stage::EnableApis),
        InputResult::Cancelled => return Err(ctx.cancelled()),
    };
    let secret = match ctx
        .wizard()?
        .show_secret_input("OAuth Client Secret", "Paste the Client Secret")
        .map_err(tui_err)?
    {
        InputResult::Confirmed(v) if !v.trim().is_empty() => {
            SecretString::from(v.trim().to_string())
        }
        InputResult::Confirmed(_) => {
            ctx.finish_wizard()?;
            return Err(GwsError::Validation(
                "the client secret cannot be empty".into(),
            ));
        }
        InputResult::GoBack => return Ok(Stage::EnableApis),
        InputResult::Cancelled => return Err(ctx.cancelled()),
    };
    crate::auth::client_config::save_client_config(
        &ctx.client_path,
        &client_id,
        &secret,
        Some(&ctx.project_id),
    )
    .map_err(crate::auth::to_gws_error)?;
    ctx.step(4, StepStatus::Done("configured".into()))?;
    Ok(Stage::Finish)
}

fn setup_summary(ctx: &Ctx, status: &str, message: &str) -> serde_json::Value {
    json!({
        "status": status,
        "message": message,
        "account": ctx.account,
        "project": ctx.project_id,
        "apis_enabled": ctx.enable.enabled,
        "apis_already_enabled": ctx.enable.skipped,
        "apis_failed": ctx.enable.failed.iter().map(|(api, err)| json!({"api": api, "error": err})).collect::<Vec<_>>(),
        "consent_screen_note": ctx.consent_note,
        "client_config": ctx.client_path.display().to_string(),
        "next_steps": ctx.manual_steps,
    })
}

pub(crate) fn should_offer_login_prompt(
    interactive: bool,
    dry_run: bool,
    login_requested: bool,
    stdout_is_terminal: bool,
) -> bool {
    interactive && !dry_run && !login_requested && stdout_is_terminal
}

fn prompt_login_after_setup() -> Result<bool, GwsError> {
    use std::io::Write;
    let mut input = String::new();
    loop {
        crate::output::eprint_text("Run `gwsr auth login` now? [Y/n]: ");
        std::io::stderr()
            .flush()
            .map_err(|e| GwsError::Validation(format!("cannot write prompt: {e}")))?;
        input.clear();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| GwsError::Validation(format!("cannot read input: {e}")))?;
        match input.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => crate::output::eprint_line("Please answer 'y' or 'n'."),
        }
    }
}

/// Run the whole setup flow and print a JSON summary.
///
/// # Errors
///
/// gcloud/TUI failures, cancellation, and (non-interactive) the manual
/// OAuth-client instructions.
pub async fn run_setup(args: &[String]) -> Result<(), GwsError> {
    let Some(opts) = parse_setup_args(args)? else {
        return Ok(());
    };
    let interactive = !opts.non_interactive
        && !opts.dry_run
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();
    if opts.dry_run {
        crate::output::eprint_line("DRY RUN: no changes will be made\n");
    }
    let client_path =
        crate::auth::client_config::client_config_path().map_err(crate::auth::to_gws_error)?;
    let wizard = if interactive {
        Some(SetupWizard::start(&STEP_LABELS).map_err(tui_err)?)
    } else {
        None
    };
    let mut ctx = Ctx {
        wizard,
        interactive,
        opts,
        gcloud: Gcloud::default(),
        client_path,
        account: String::new(),
        project_id: String::new(),
        api_ids: Vec::new(),
        enable: EnableResult::default(),
        consent_note: None,
        manual_steps: None,
    };

    let mut stage = Stage::CheckGcloud;
    let outcome: Result<(), GwsError> = async {
        loop {
            stage = match stage {
                Stage::CheckGcloud => stage_check_gcloud(&mut ctx).await?,
                Stage::Account => stage_account(&mut ctx).await?,
                Stage::Project => stage_project(&mut ctx).await?,
                Stage::EnableApis => stage_enable_apis(&mut ctx).await?,
                Stage::ConfigureOauth => stage_configure_oauth(&mut ctx).await?,
                Stage::Finish => return Ok(()),
            };
        }
    }
    .await;
    // Always restore the terminal, even on errors.
    let restore = ctx.finish_wizard();
    outcome?;
    restore?;

    if ctx.opts.dry_run {
        return crate::auth::commands::print_json(&json!({
            "status": "dry_run",
            "message": "No changes were made.",
            "account": ctx.account,
            "project": ctx.project_id,
            "apis_would_enable": ctx.api_ids,
        }));
    }

    if let Some(steps) = &ctx.manual_steps {
        crate::output::eprint_line(&format!("\n{steps}"));
        return crate::auth::commands::print_json(&setup_summary(
            &ctx,
            "action_required",
            "Project and APIs are ready; create a Desktop OAuth client manually (see next_steps).",
        ));
    }

    let run_login = ctx.opts.login
        || (should_offer_login_prompt(
            ctx.interactive,
            ctx.opts.dry_run,
            ctx.opts.login,
            std::io::stdout().is_terminal(),
        ) && prompt_login_after_setup()?);
    let message = if run_login {
        "Setup complete. Starting `gwsr auth login`..."
    } else {
        "Setup complete. Run `gwsr auth login` to sign in."
    };
    crate::auth::commands::print_json(&setup_summary(&ctx, "success", message))?;
    if run_login {
        crate::auth::commands::run_login(&[]).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    fn items(labels: &[&str]) -> Vec<SelectItem> {
        labels.iter().map(|l| item(l, "", false)).collect()
    }

    fn simulate(items: Vec<SelectItem>, keys: &[KeyCode], multi: bool) -> Vec<SelectItem> {
        let mut state = tui::PickerState::new("t", "", items, multi);
        for key in keys {
            if let Some(PickerResult::Confirmed(r)) = state.handle_key(*key) {
                return r;
            }
        }
        panic!("keys did not confirm");
    }

    #[test]
    fn parse_args() {
        let o = parse_setup_args(&[]).unwrap().unwrap();
        assert_eq!(
            o,
            SetupOptions {
                project: None,
                dry_run: false,
                login: false,
                non_interactive: false,
            }
        );
        let o = parse_setup_args(&["--project=p".into(), "--dry-run".into(), "--login".into()])
            .unwrap()
            .unwrap();
        assert_eq!(o.project.as_deref(), Some("p"));
        assert!(o.dry_run && o.login);
        let o = parse_setup_args(&["--non-interactive".into()])
            .unwrap()
            .unwrap();
        assert!(o.non_interactive);
        assert!(parse_setup_args(&["--verbose".into()]).is_err());
        assert!(parse_setup_args(&["--help".into()]).unwrap().is_none());
    }

    #[test]
    fn login_prompt_rules() {
        assert!(should_offer_login_prompt(true, false, false, true));
        assert!(!should_offer_login_prompt(true, false, true, true));
        assert!(!should_offer_login_prompt(false, false, false, true));
        assert!(!should_offer_login_prompt(true, true, false, true));
        assert!(!should_offer_login_prompt(true, false, false, false));
    }

    #[test]
    fn account_choices_via_keys() {
        let list = items(&[LOGIN_NEW_ACCOUNT, "a@x.com", "b@x.com"]);
        let r = simulate(list.clone(), &[KeyCode::Down, KeyCode::Enter], false);
        assert_eq!(
            account_choice(&r),
            AccountChoice::Existing("a@x.com".into())
        );
        let r = simulate(list, &[KeyCode::Enter], false);
        assert_eq!(account_choice(&r), AccountChoice::LoginNew);
        assert_eq!(account_choice(&items(&["x"])), AccountChoice::None);
    }

    #[test]
    fn project_choices_via_keys() {
        let list = items(&[CREATE_PROJECT, ENTER_PROJECT, "p1", "p2"]);
        let r = simulate(list.clone(), &[KeyCode::Enter], false);
        assert_eq!(project_choice(&r), ProjectChoice::Create);
        let r = simulate(list.clone(), &[KeyCode::Down, KeyCode::Enter], false);
        assert_eq!(project_choice(&r), ProjectChoice::Enter);
        let r = simulate(
            list,
            &[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Enter],
            false,
        );
        assert_eq!(project_choice(&r), ProjectChoice::Existing("p2".into()));
    }

    #[test]
    fn api_choices_via_keys() {
        let list: Vec<SelectItem> = apis::WORKSPACE_APIS
            .iter()
            .map(|a| item(a.name, a.id, false))
            .collect();
        let r = simulate(list.clone(), &[KeyCode::Enter], true);
        assert!(api_choice(&r).is_empty());
        let r = simulate(
            list.clone(),
            &[
                KeyCode::Char(' '),
                KeyCode::Down,
                KeyCode::Down,
                KeyCode::Char(' '),
                KeyCode::Enter,
            ],
            true,
        );
        assert_eq!(
            api_choice(&r),
            vec![
                apis::WORKSPACE_APIS[0].id.to_string(),
                apis::WORKSPACE_APIS[2].id.to_string()
            ]
        );
        let r = simulate(list, &[KeyCode::Char('a'), KeyCode::Enter], true);
        assert_eq!(api_choice(&r).len(), apis::WORKSPACE_APIS.len());
    }
}
