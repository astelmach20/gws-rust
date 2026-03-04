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

use super::clasp_config;
use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use anyhow::Context;
use clap::{Arg, ArgMatches, Command};
use serde_json::json;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub struct ScriptHelper;

impl Helper for ScriptHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        // +push
        cmd = cmd.subcommand(
            Command::new("+push")
                .about("[Helper] Upload local files to an Apps Script project")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID (reads .clasp.json if omitted)")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("dir")
                        .long("dir")
                        .help("Directory containing script files (reads .clasp.json rootDir, or defaults to current dir)")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +push --script SCRIPT_ID
  gws script +push --script SCRIPT_ID --dir ./src
  gws script +push                        # uses .clasp.json

TIPS:
  Supports .gs, .js, .html, and appsscript.json files.
  Skips hidden files and node_modules automatically.
  This replaces ALL files in the project.",
                ),
        );

        // +pull
        cmd = cmd.subcommand(
            Command::new("+pull")
                .about("[Helper] Download project files to local directory")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID (reads .clasp.json if omitted)")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("dir")
                        .long("dir")
                        .help("Output directory (reads .clasp.json rootDir, or defaults to current dir)")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +pull --script SCRIPT_ID
  gws script +pull --script SCRIPT_ID --dir ./src
  gws script +pull                        # uses .clasp.json

FILES CREATED:
  SERVER_JS  → {name}.gs
  HTML       → {name}.html
  JSON       → appsscript.json",
                ),
        );

        // +open
        cmd = cmd.subcommand(
            Command::new("+open")
                .about("[Helper] Open the script editor in your browser")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID (reads .clasp.json if omitted)")
                        .value_name("ID"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +open --script SCRIPT_ID
  gws script +open                        # uses .clasp.json",
                ),
        );

        // +run
        cmd = cmd.subcommand(
            Command::new("+run")
                .about("[Helper] Execute a function in the script")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID (reads .clasp.json if omitted)")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("function")
                        .long("function")
                        .help("Function name to execute")
                        .required(true)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("dev-mode")
                        .long("dev-mode")
                        .help("Run the script in dev mode (HEAD deployment)")
                        .action(clap::ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +run --script SCRIPT_ID --function main
  gws script +run --function main         # uses .clasp.json
  gws script +run --function main --dev-mode

SETUP REQUIREMENTS:
  1. Auth with cloud-platform scope: gws auth login
  2. Link the script to your OAuth client's GCP project:
     Open the script editor (gws apps-script +open) → Project Settings →
     Change GCP project → enter your project number.
  3. Add to appsscript.json: \"executionApi\": {\"access\": \"MYSELF\"}",
                ),
        );

        // +logs
        cmd = cmd.subcommand(
            Command::new("+logs")
                .about("[Helper] View execution logs for the script")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID (reads .clasp.json if omitted)")
                        .value_name("ID"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +logs --script SCRIPT_ID
  gws script +logs                        # uses .clasp.json

TIPS:
  Shows recent script executions and their status.
  Use --format table for a readable summary.",
                ),
        );

        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(sub) = matches.subcommand_matches("+push") {
                return handle_push(doc, sub).await.map(|_| true);
            }
            if let Some(sub) = matches.subcommand_matches("+pull") {
                return handle_pull(doc, sub).await.map(|_| true);
            }
            if let Some(sub) = matches.subcommand_matches("+open") {
                return handle_open(sub).await.map(|_| true);
            }
            if let Some(sub) = matches.subcommand_matches("+run") {
                return handle_run(doc, sub).await.map(|_| true);
            }
            if let Some(sub) = matches.subcommand_matches("+logs") {
                return handle_logs(doc, sub).await.map(|_| true);
            }
            Ok(false)
        })
    }
}

// ---------------------------------------------------------------------------
// +push
// ---------------------------------------------------------------------------

async fn handle_push(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let script_id = clasp_config::resolve_script_id(matches.get_one::<String>("script"))?;
    let dir = clasp_config::resolve_dir(matches.get_one::<String>("dir"))?;

    let mut files = Vec::new();
    visit_dirs(&dir, &mut files)?;

    if files.is_empty() {
        return Err(GwsError::Validation(format!(
            "No eligible files found in '{}'",
            dir.display()
        )));
    }

    let projects_res = doc
        .resources
        .get("projects")
        .ok_or_else(|| GwsError::Discovery("Resource 'projects' not found".to_string()))?;
    let update_method = projects_res.methods.get("updateContent").ok_or_else(|| {
        GwsError::Discovery("Method 'projects.updateContent' not found".to_string())
    })?;

    let body = json!({ "files": files });
    let body_str = body.to_string();

    let scopes: Vec<&str> = update_method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) => (None, executor::AuthMethod::None),
    };

    let params = json!({ "scriptId": script_id });
    let params_str = params.to_string();

    executor::execute_method(
        doc,
        update_method,
        Some(&params_str),
        Some(&body_str),
        token.as_deref(),
        auth_method,
        None,
        None,
        matches.get_flag("dry-run"),
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// +pull
// ---------------------------------------------------------------------------

async fn handle_pull(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let script_id = clasp_config::resolve_script_id(matches.get_one::<String>("script"))?;
    let output_dir = matches
        .get_one::<String>("dir")
        .map(|d| crate::validate::validate_safe_output_dir(d))
        .transpose()?
        .unwrap_or_else(|| {
            clasp_config::resolve_dir(None).unwrap_or_else(|_| std::env::current_dir().unwrap())
        });

    let projects_res = doc
        .resources
        .get("projects")
        .ok_or_else(|| GwsError::Discovery("Resource 'projects' not found".to_string()))?;
    let get_content_method = projects_res
        .methods
        .get("getContent")
        .ok_or_else(|| GwsError::Discovery("Method 'projects.getContent' not found".to_string()))?;

    let scopes: Vec<&str> = get_content_method
        .scopes
        .iter()
        .map(|s| s.as_str())
        .collect();
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) => (None, executor::AuthMethod::None),
    };

    let params = json!({ "scriptId": script_id });
    let params_str = params.to_string();

    let response = executor::execute_method(
        doc,
        get_content_method,
        Some(&params_str),
        None,
        token.as_deref(),
        auth_method,
        None,
        None,
        false,
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        true, // capture output
    )
    .await?;

    let response_value =
        response.ok_or_else(|| GwsError::Validation("No response from getContent".to_string()))?;

    let files = response_value
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or_else(|| GwsError::Validation("Response missing 'files' array".to_string()))?;

    // Create output directory if needed
    fs::create_dir_all(&output_dir).map_err(|e| {
        GwsError::Validation(format!(
            "Failed to create output directory '{}': {e}",
            output_dir.display()
        ))
    })?;

    let canonical_output = output_dir.canonicalize().map_err(|e| {
        GwsError::Validation(format!(
            "Failed to canonicalize output directory '{}': {e}",
            output_dir.display()
        ))
    })?;

    let mut written = 0;
    for file in files {
        let name = file.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let file_type = file.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let source = file.get("source").and_then(|s| s.as_str()).unwrap_or("");

        // Validate the filename from the API
        crate::validate::validate_script_filename(name)?;

        let extension = file_type_to_extension(file_type);
        if extension.is_empty() {
            continue; // Skip unknown types
        }

        let filename = format!("{name}{extension}");
        let file_path = canonical_output.join(&filename);

        // Final containment check — ensure resolved path is inside output dir
        let canonical_file = if file_path.exists() {
            file_path
                .canonicalize()
                .map_err(|e| GwsError::Validation(format!("Failed to resolve path: {e}")))?
        } else {
            // For new files, the parent exists (we created it), so this is safe
            file_path.clone()
        };

        if !canonical_file.starts_with(&canonical_output) {
            return Err(GwsError::Validation(format!(
                "File '{}' would be written outside output directory — refusing",
                filename
            )));
        }

        fs::write(&file_path, source).map_err(|e| {
            GwsError::Validation(format!("Failed to write '{}': {e}", file_path.display()))
        })?;
        written += 1;
    }

    eprintln!("Pulled {written} files to {}", canonical_output.display());
    Ok(())
}

/// Maps Apps Script file type to local file extension.
fn file_type_to_extension(file_type: &str) -> &str {
    match file_type {
        "SERVER_JS" => ".gs",
        "HTML" => ".html",
        "JSON" => ".json",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// +open
// ---------------------------------------------------------------------------

async fn handle_open(matches: &ArgMatches) -> Result<(), GwsError> {
    let script_id = clasp_config::resolve_script_id(matches.get_one::<String>("script"))?;

    let url = format!("https://script.google.com/d/{}/edit", script_id);
    eprintln!("Opening {url}");

    open::that(&url).map_err(|e| GwsError::Validation(format!("Failed to open browser: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// +run
// ---------------------------------------------------------------------------

async fn handle_run(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let script_id = clasp_config::resolve_script_id(matches.get_one::<String>("script"))?;
    let function_name = matches.get_one::<String>("function").unwrap();
    let dev_mode = matches.get_flag("dev-mode");

    let scripts_res = doc
        .resources
        .get("scripts")
        .ok_or_else(|| GwsError::Discovery("Resource 'scripts' not found".to_string()))?;
    let run_method = scripts_res
        .methods
        .get("run")
        .ok_or_else(|| GwsError::Discovery("Method 'scripts.run' not found".to_string()))?;

    let mut body = json!({
        "function": function_name,
    });
    if dev_mode {
        body["devMode"] = json!(true);
    }
    let body_str = body.to_string();

    // scripts.run is special: the discovery doc lists scopes of services the
    // *script* uses (spreadsheets, drive, mail, etc), not a dedicated "run" scope.
    // cloud-platform is a superset that covers all of them.
    let (token, auth_method) =
        match auth::get_token(&["https://www.googleapis.com/auth/cloud-platform"]).await {
            Ok(t) => (Some(t), executor::AuthMethod::OAuth),
            Err(_) => (None, executor::AuthMethod::None),
        };

    let params = json!({ "scriptId": script_id });
    let params_str = params.to_string();

    let result = executor::execute_method(
        doc,
        run_method,
        Some(&params_str),
        Some(&body_str),
        token.as_deref(),
        auth_method,
        None,
        None,
        matches.get_flag("dry-run"),
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await;

    if let Err(ref e) = result {
        let msg = e.to_string();
        let gcp_project_hint = extract_gcp_project_number();

        if msg.contains("authentication scopes") {
            eprintln!(
                "\n\x1b[33mHint:\x1b[0m scripts.run requires scopes matching the services your \
                 script uses.\n\
                 Re-run \x1b[1mgws auth login\x1b[0m and include the \x1b[1mcloud-platform\x1b[0m scope."
            );
        } else if msg.contains("does not have permission") || msg.contains("403") {
            eprintln!(
                "\n\x1b[33mHint:\x1b[0m The script must be linked to your OAuth client's GCP project.{gcp}\n\
                 1. Open the script editor: \x1b[1mgws apps-script +open\x1b[0m\n\
                 2. Go to Project Settings (gear icon)\n\
                 3. Under 'Google Cloud Platform (GCP) Project', click 'Change project'\n\
                 4. Enter your GCP project number and click 'Set project'\n\n\
                 Also ensure appsscript.json includes: \x1b[1m\"executionApi\": {{\"access\": \"MYSELF\"}}\x1b[0m",
                gcp = gcp_project_hint,
            );
        } else if msg.contains("not found") || msg.contains("404") {
            eprintln!(
                "\n\x1b[33mHint:\x1b[0m The script was not found. Possible causes:\n\
                 • The script is not linked to a GCP project{gcp}\n\
                   (see \x1b[1mgws apps-script +open\x1b[0m → Project Settings)\n\
                 • The script has no API-executable deployment (create one with:\n\
                   \x1b[1mgws apps-script projects versions create\x1b[0m then\n\
                   \x1b[1mgws apps-script projects deployments create\x1b[0m)\n\
                 • Use \x1b[1m--dev-mode\x1b[0m to run the latest saved code without deploying",
                gcp = gcp_project_hint,
            );
        }
    }
    result?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the GCP project number from the stored OAuth client ID.
///
/// Client IDs have the format `{project_number}-{hash}.apps.googleusercontent.com`.
/// Returns a formatted string like `\n   GCP project number: 388781138000` or empty string.
fn extract_gcp_project_number() -> String {
    let enc_path = crate::credential_store::encrypted_credentials_path();
    if !enc_path.exists() {
        return String::new();
    }

    let json_str = match crate::credential_store::load_encrypted_from_path(&enc_path) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let creds: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    if let Some(client_id) = creds.get("client_id").and_then(|v| v.as_str()) {
        if let Some(project_num) = client_id.split('-').next() {
            if project_num.chars().all(|c| c.is_ascii_digit()) && !project_num.is_empty() {
                return format!(
                    "\n   Your OAuth client's GCP project number: \x1b[1m{project_num}\x1b[0m"
                );
            }
        }
    }

    String::new()
}

// ---------------------------------------------------------------------------
// +logs
// ---------------------------------------------------------------------------

async fn handle_logs(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let script_id = clasp_config::resolve_script_id(matches.get_one::<String>("script"))?;

    let processes_res = doc
        .resources
        .get("processes")
        .ok_or_else(|| GwsError::Discovery("Resource 'processes' not found".to_string()))?;
    let list_method = processes_res
        .methods
        .get("listScriptProcesses")
        .ok_or_else(|| {
            GwsError::Discovery("Method 'processes.listScriptProcesses' not found".to_string())
        })?;

    let scopes: Vec<&str> = list_method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) => (None, executor::AuthMethod::None),
    };

    let params = json!({ "scriptId": script_id });
    let params_str = params.to_string();

    executor::execute_method(
        doc,
        list_method,
        Some(&params_str),
        None,
        token.as_deref(),
        auth_method,
        None,
        None,
        matches.get_flag("dry-run"),
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

fn visit_dirs(dir: &Path, files: &mut Vec<serde_json::Value>) -> Result<(), GwsError> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).context("Failed to read dir")? {
            let entry = entry.context("Failed to read entry")?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, files)?;
            } else if let Some(file_obj) = process_file(&path)? {
                files.push(file_obj);
            }
        }
    }
    Ok(())
}

fn process_file(path: &Path) -> Result<Option<serde_json::Value>, GwsError> {
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Skip hidden files, node_modules, .git, etc. (basic filtering)
    if filename.starts_with('.') || path.components().any(|c| c.as_os_str() == "node_modules") {
        return Ok(None);
    }

    let (type_val, name_val) = match extension {
        "gs" | "js" => (
            "SERVER_JS",
            filename.trim_end_matches(".js").trim_end_matches(".gs"),
        ),
        "html" => ("HTML", filename.trim_end_matches(".html")),
        "json" => {
            if filename == "appsscript.json" {
                ("JSON", "appsscript")
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    let content = fs::read_to_string(path).map_err(|e| {
        GwsError::Validation(format!("Failed to read file '{}': {}", path.display(), e))
    })?;

    Ok(Some(json!({
        "name": name_val,
        "type": type_val,
        "source": content
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_process_file_server_js() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("code.gs");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "function foo() {{}}").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "code");
        assert_eq!(result["type"], "SERVER_JS");
        assert_eq!(
            result["source"].as_str().unwrap().trim(),
            "function foo() {}"
        );
    }

    #[test]
    fn test_process_file_html() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("index.html");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "<html></html>").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "index");
        assert_eq!(result["type"], "HTML");
    }

    #[test]
    fn test_process_file_appsscript_json() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("appsscript.json");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{{}}").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "appsscript");
        assert_eq!(result["type"], "JSON");
    }

    #[test]
    fn test_process_file_ignored() {
        let dir = tempdir().unwrap();

        // Random JSON
        let p1 = dir.path().join("other.json");
        File::create(&p1).unwrap();
        assert!(process_file(&p1).unwrap().is_none());

        // Hidden file
        let p2 = dir.path().join(".hidden.gs");
        File::create(&p2).unwrap();
        assert!(process_file(&p2).unwrap().is_none());

        // node_modules
        let node_modules = dir.path().join("node_modules");
        fs::create_dir(&node_modules).unwrap();
        let p3 = node_modules.join("dep.gs");
        File::create(&p3).unwrap();
        assert!(process_file(&p3).unwrap().is_none());
    }

    #[test]
    fn test_visit_dirs() {
        let dir = tempdir().unwrap();

        // Root file
        let f1 = dir.path().join("root.gs");
        File::create(&f1).unwrap();

        // Subdir file
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        let f2 = sub.join("utils.js");
        File::create(&f2).unwrap();

        // Ignored file
        let f3 = dir.path().join("ignore.txt");
        File::create(&f3).unwrap();

        let mut files = Vec::new();
        visit_dirs(dir.path(), &mut files).unwrap();

        assert_eq!(files.len(), 2);

        let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();

        assert!(names.contains(&"root"));
        assert!(names.contains(&"utils"));
    }

    #[test]
    fn test_file_type_to_extension() {
        assert_eq!(file_type_to_extension("SERVER_JS"), ".gs");
        assert_eq!(file_type_to_extension("HTML"), ".html");
        assert_eq!(file_type_to_extension("JSON"), ".json");
        assert_eq!(file_type_to_extension("UNKNOWN"), "");
        assert_eq!(file_type_to_extension(""), "");
    }
}
