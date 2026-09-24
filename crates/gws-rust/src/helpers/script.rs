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

//! Apps Script helpers: `+push`, `+pull`, `+run`, `+logs`.
//!
//! Script IDs are encoded per RFC 3986 so `-`/`_` stay literal (#842).

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, many, optional, required};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

const SCOPE_PROJECTS: &str = "https://www.googleapis.com/auth/script.projects";
const SCOPE_PROJECTS_READONLY: &str = "https://www.googleapis.com/auth/script.projects.readonly";
const SCOPE_PROCESSES: &str = "https://www.googleapis.com/auth/script.processes";

pub struct ScriptHelper;

fn script_id_arg() -> Arg {
    Arg::new("script-id")
        .long("script-id")
        .help("Apps Script project ID")
        .required(true)
        .value_name("ID")
}

fn dir_arg(help: &'static str) -> Arg {
    Arg::new("dir")
        .long("dir")
        .help(help)
        .default_value(".")
        .value_name("DIR")
}

impl Helper for ScriptHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(with_yes(
            Command::new("+push")
                .about("[Helper] Replace a project's files with local files")
                .arg(script_id_arg())
                .arg(dir_arg("Directory with the script files"))
                .after_help(
                    "\
EXAMPLES:
  gwsr script +push --script-id SCRIPT_ID --yes
  gwsr script +push --script-id SCRIPT_ID --dir ./src --yes

TIPS:
  Uploads .gs/.js (server code), .html and appsscript.json (required).
  Files in sub-directories keep their path (e.g. lib/util.gs -> lib/util).
  Hidden files/directories, node_modules and symlinks are skipped.
  Destructive: this REPLACES ALL files in the project, so it requires --yes
  (or a confirmation prompt on a terminal).",
                ),
        ))
        .subcommand(
            Command::new("+pull")
                .about("[Helper] Download a project's files into a local directory")
                .arg(script_id_arg())
                .arg(dir_arg("Destination directory (created if missing)"))
                .arg(
                    Arg::new("overwrite")
                        .long("overwrite")
                        .help("Replace existing local files")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr script +pull --script-id SCRIPT_ID --dir ./src
  gwsr script +pull --script-id SCRIPT_ID --dir ./src --overwrite

TIPS:
  Server code is written as .gs, HTML as .html, the manifest as appsscript.json.
  Nothing is written if any target file exists and --overwrite is not given.",
                ),
        )
        .subcommand(with_yes(
            Command::new("+run")
                .about("[Helper] Run a function in a deployed Apps Script project")
                .arg(script_id_arg())
                .arg(
                    Arg::new("function")
                        .long("function")
                        .help("Function name")
                        .required(true)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("args")
                        .long("args")
                        .help("Function arguments as a JSON array, e.g. '[\"a\", 2]'")
                        .value_name("JSON"),
                )
                .arg(
                    Arg::new("dev-mode")
                        .long("dev-mode")
                        .help("Run the most recently saved code instead of the deployed version (owner only)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("scope")
                        .long("scope")
                        .help("OAuth scope the function needs (repeatable; default: the scopes listed by the Execution API)")
                        .action(ArgAction::Append)
                        .value_name("SCOPE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr script +run --script-id SCRIPT_ID --function main
  gwsr script +run --script-id SCRIPT_ID --function add --args '[1, 2]' --dev-mode

TIPS:
  The project must be deployed as an API executable and share a Cloud
  project with your OAuth client. Prints the function's return value.
  A script error fails the command with the script's message and stack.",
                ),
        ))
        .subcommand(
            Command::new("+logs")
                .about("[Helper] List recent executions of a project")
                .arg(script_id_arg())
                .arg(
                    Arg::new("function")
                        .long("function")
                        .help("Only executions of this function")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum executions to list")
                        .default_value("50")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr script +logs --script-id SCRIPT_ID
  gwsr script +logs --script-id SCRIPT_ID --function main --limit 10

TIPS:
  Lists executions (function, status, start time, duration). console.log
  output is stored in Cloud Logging, not returned by this API.",
                ),
        )
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            let Some((name, m)) = matches.subcommand() else {
                return Ok(false);
            };
            if !["+push", "+pull", "+run", "+logs"].contains(&name) {
                return Ok(false);
            }
            let dry = crate::args::dry_run(m)?;
            let script_id = required(m, "script-id")?;
            let (api, value) = match name {
                "+push" => {
                    let dir = crate::validate::validate_safe_dir_path(required(m, "dir")?)?;
                    let files = collect_files(&dir)?;
                    confirm::confirm(
                        m,
                        Impact::Destructive,
                        &format!(
                            "replace ALL files of script {script_id} with {} local file(s)",
                            files.len()
                        ),
                    )?;
                    let api = Api::new(doc, &[SCOPE_PROJECTS], dry, sanitize).await?;
                    let v = push(&api, script_id, files).await?;
                    (api, v)
                }
                "+pull" => {
                    let dir = crate::validate::validate_safe_output_dir(required(m, "dir")?)?;
                    let api = Api::new(doc, &[SCOPE_PROJECTS_READONLY], dry, sanitize).await?;
                    let v = pull(&api, script_id, &dir, flag(m, "overwrite")?).await?;
                    (api, v)
                }
                "+run" => {
                    let function = required(m, "function")?;
                    let args = parse_args(optional(m, "args")?)?;
                    confirm::confirm(
                        m,
                        Impact::Outbound,
                        &format!(
                            "run {function}() in script {script_id} with your account's permissions"
                        ),
                    )?;
                    let mut scopes = many(m, "scope")?;
                    if scopes.is_empty() {
                        scopes = run_scopes(doc);
                    }
                    let scope_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
                    let api = Api::new(doc, &scope_refs, dry, sanitize).await?;
                    let v = run(&api, script_id, function, args, flag(m, "dev-mode")?).await?;
                    (api, v)
                }
                "+logs" => {
                    let limit = http::limit(m, "limit")?;
                    let api = Api::new(doc, &[SCOPE_PROCESSES], dry, sanitize).await?;
                    let v = logs(&api, script_id, optional(m, "function")?, limit).await?;
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

fn project_url(api: &Api, script_id: &str, suffix: &str) -> String {
    api.url(&format!(
        "v1/projects/{}{suffix}",
        encode_path_segment(script_id)
    ))
}

// ── +push ────────────────────────────────────────────────────────────

fn skip_name(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

/// Collect pushable files under `root`, keyed by Apps Script file name.
fn collect_files(root: &Path) -> Result<Vec<Value>, GwsError> {
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| {
            GwsError::Validation(format!("Failed to read directory '{}': {e}", dir.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                GwsError::Validation(format!("Failed to read entry in '{}': {e}", dir.display()))
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if skip_name(&name) {
                continue;
            }
            let ft = entry.file_type().map_err(|e| {
                GwsError::Validation(format!("Failed to stat '{}': {e}", path.display()))
            })?;
            if ft.is_symlink() {
                tracing::warn!("skipping symlink '{}'", path.display());
                continue;
            }
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            if let Some((script_name, file)) = process_file(root, &path)? {
                if let Some(existing) = files.get(&script_name) {
                    return Err(GwsError::Validation(format!(
                        "Two local files map to the Apps Script file '{script_name}' ({} and {}); rename one",
                        existing["_path"].as_str().unwrap_or("?"),
                        path.display()
                    )));
                }
                files.insert(script_name, file);
            }
        }
    }
    if files.is_empty() {
        return Err(GwsError::Validation(format!(
            "No .gs/.js/.html/appsscript.json files found in '{}'",
            root.display()
        )));
    }
    if !files.contains_key("appsscript") {
        return Err(GwsError::Validation(format!(
            "'{}' has no appsscript.json manifest; Apps Script requires it (use +pull to fetch the current one)",
            root.display()
        )));
    }
    Ok(files
        .into_values()
        .map(|mut f| {
            if let Some(o) = f.as_object_mut() {
                o.remove("_path");
            }
            f
        })
        .collect())
}

/// Map one local file to an Apps Script file object, or `None` to skip it.
fn process_file(root: &Path, path: &Path) -> Result<Option<(String, Value)>, GwsError> {
    let rel = path.strip_prefix(root).map_err(http::other_err)?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let stem = rel_str
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| rel_str.clone());
    let (type_val, name) = match extension {
        "gs" | "js" => ("SERVER_JS", stem),
        "html" => ("HTML", stem),
        "json" if rel_str == "appsscript.json" => ("JSON", "appsscript".to_string()),
        _ => return Ok(None),
    };
    let source = fs::read_to_string(path)
        .map_err(|e| GwsError::Validation(format!("Failed to read '{}': {e}", path.display())))?;
    Ok(Some((
        name.clone(),
        json!({ "name": name, "type": type_val, "source": source, "_path": path.display().to_string() }),
    )))
}

async fn push(api: &Api, script_id: &str, files: Vec<Value>) -> Result<Value, GwsError> {
    let names: Vec<Value> = files.iter().map(|f| f["name"].clone()).collect();
    api.send(
        ApiRequest::put(project_url(api, script_id, "/content")).json(json!({ "files": files })),
    )
    .await?;
    Ok(json!({ "scriptId": script_id, "files": names }))
}

// ── +pull ────────────────────────────────────────────────────────────

/// Local relative path for a remote Apps Script file.
fn local_path_for(name: &str, file_type: &str) -> Result<PathBuf, GwsError> {
    let ext = match file_type {
        "SERVER_JS" => "gs",
        "HTML" => "html",
        "JSON" => "json",
        other => {
            return Err(GwsError::Validation(format!(
                "Unsupported Apps Script file type '{other}' for '{name}'"
            )));
        }
    };
    let mut path = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains('\\')
            || part.chars().any(char::is_control)
        {
            return Err(GwsError::Validation(format!(
                "Refusing unsafe Apps Script file name '{name}'"
            )));
        }
        path.push(part);
    }
    path.set_extension(ext);
    Ok(path)
}

async fn pull(api: &Api, script_id: &str, dir: &Path, overwrite: bool) -> Result<Value, GwsError> {
    let content = api
        .send(ApiRequest::get(project_url(api, script_id, "/content")))
        .await?;
    if api.is_dry_run() {
        api.plan_note(json!({ "note": "each project file is written under the directory", "dir": dir.display().to_string() }))?;
        return Ok(Value::Null);
    }
    let files = content
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| http::other_err(anyhow::anyhow!("Project content has no files")))?;
    let mut planned = Vec::new();
    for f in files {
        let name = f.get("name").and_then(Value::as_str).unwrap_or("");
        let file_type = f.get("type").and_then(Value::as_str).unwrap_or("");
        let source = f.get("source").and_then(Value::as_str).unwrap_or("");
        let path = dir.join(local_path_for(name, file_type)?);
        if !overwrite && path.exists() {
            return Err(GwsError::Validation(format!(
                "'{}' already exists; pass --overwrite to replace local files (nothing was written)",
                path.display()
            )));
        }
        planned.push((path, source));
    }
    let mut written = Vec::new();
    for (path, source) in planned {
        crate::output_file::write_atomic(&path, source.as_bytes(), overwrite).await?;
        written.push(path.display().to_string());
    }
    Ok(json!({ "scriptId": script_id, "files": written }))
}

// ── +run ─────────────────────────────────────────────────────────────

fn parse_args(raw: Option<&str>) -> Result<Option<Value>, GwsError> {
    raw.map(|s| {
        let v: Value = serde_json::from_str(s)
            .map_err(|e| GwsError::Validation(format!("--args is not valid JSON: {e}")))?;
        if !v.is_array() {
            return Err(GwsError::Validation("--args must be a JSON array".into()));
        }
        Ok(v)
    })
    .transpose()
}

/// Scopes listed by the Discovery document for `scripts.run`.
fn run_scopes(doc: &crate::discovery::RestDescription) -> Vec<String> {
    doc.resources
        .get("scripts")
        .and_then(|r| r.methods.get("run"))
        .map(|m| m.scopes.clone())
        .unwrap_or_default()
}

fn script_error(details: &Value) -> GwsError {
    let detail = details.pointer("/details/0").unwrap_or(details);
    let kind = detail
        .get("errorType")
        .and_then(Value::as_str)
        .unwrap_or("ScriptError");
    let message = detail
        .get("errorMessage")
        .or_else(|| details.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    let stack: Vec<String> = detail
        .get("scriptStackTraceElements")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|e| {
                    format!(
                        "  at {} (line {})",
                        e.get("function").and_then(Value::as_str).unwrap_or("?"),
                        e.get("lineNumber")
                            .map(Value::to_string)
                            .unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut msg = format!("Script failed: {kind}: {message}");
    if !stack.is_empty() {
        msg.push('\n');
        msg.push_str(&stack.join("\n"));
    }
    GwsError::Api {
        code: 500,
        message: msg,
        reason: "scriptError".into(),
        enable_url: None,
    }
}

async fn run(
    api: &Api,
    script_id: &str,
    function: &str,
    args: Option<Value>,
    dev_mode: bool,
) -> Result<Value, GwsError> {
    let mut body = json!({ "function": function, "devMode": dev_mode });
    if let Some(a) = args {
        body["parameters"] = a;
    }
    let resp = api
        .send(
            ApiRequest::post(api.url(&format!(
                "v1/scripts/{}:run",
                encode_path_segment(script_id)
            )))
            .json(body),
        )
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    if let Some(err) = resp.get("error") {
        return Err(script_error(err));
    }
    Ok(json!({ "result": resp.pointer("/response/result").cloned().unwrap_or(Value::Null) }))
}

// ── +logs ────────────────────────────────────────────────────────────

async fn logs(
    api: &Api,
    script_id: &str,
    function: Option<&str>,
    limit: Option<usize>,
) -> Result<Value, GwsError> {
    let page_size = limit.unwrap_or(50).min(200).to_string();
    let req = ApiRequest::get(api.url("v1/processes:listScriptProcesses"))
        .query("scriptId", script_id)
        .query("pageSize", page_size)
        .query_opt("scriptProcessFilter.functionName", function);
    let page = api.paginate(req, "processes", limit).await?;
    Ok(page.into_json("processes"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write(p: &Path, s: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, s).unwrap();
    }

    #[test]
    fn collect_files_maps_types_and_paths() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("appsscript.json"), "{}");
        write(&dir.path().join("Code.gs"), "function a(){}");
        write(&dir.path().join("lib/util.js"), "function b(){}");
        write(&dir.path().join("index.html"), "<p/>");
        write(&dir.path().join("README.md"), "x");
        write(&dir.path().join(".hidden/x.gs"), "x");
        write(&dir.path().join("node_modules/dep.gs"), "x");
        let files = collect_files(dir.path()).unwrap();
        let mut names: Vec<(String, String)> = files
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap().into(),
                    f["type"].as_str().unwrap().into(),
                )
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                ("Code".into(), "SERVER_JS".into()),
                ("appsscript".into(), "JSON".into()),
                ("index".into(), "HTML".into()),
                ("lib/util".into(), "SERVER_JS".into()),
            ]
        );
        assert!(files.iter().all(|f| f.get("_path").is_none()));
    }

    #[test]
    fn collect_files_requires_manifest_and_rejects_collisions() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("Code.gs"), "x");
        assert!(
            collect_files(dir.path())
                .unwrap_err()
                .to_string()
                .contains("appsscript.json")
        );
        write(&dir.path().join("appsscript.json"), "{}");
        write(&dir.path().join("Code.js"), "y");
        assert!(
            collect_files(dir.path())
                .unwrap_err()
                .to_string()
                .contains("Code")
        );
    }

    #[test]
    fn local_paths_are_safe() {
        assert_eq!(
            local_path_for("lib/util", "SERVER_JS").unwrap(),
            PathBuf::from("lib/util.gs")
        );
        assert_eq!(
            local_path_for("appsscript", "JSON").unwrap(),
            PathBuf::from("appsscript.json")
        );
        assert!(local_path_for("../evil", "HTML").is_err());
        assert!(local_path_for("/abs", "HTML").is_err());
        assert!(local_path_for("x", "WEIRD").is_err());
    }

    #[tokio::test]
    async fn run_keeps_unreserved_chars_in_script_id() {
        // Regression (#842): '-' and '_' must not be percent-encoded.
        let api = dry_api("");
        run(&api, "1-ab_CD", "main", None, true).await.unwrap();
        let plan = api.planned();
        assert!(
            plan[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/v1/scripts/1-ab_CD:run")
        );
        assert_eq!(plan[0]["body"]["devMode"], true);
    }

    #[tokio::test]
    async fn run_surfaces_script_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scripts/S-1:run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "done": true,
                "error": {"code": 3, "details": [{
                    "errorType": "TypeError", "errorMessage": "x is undefined",
                    "scriptStackTraceElements": [{"function": "main", "lineNumber": 4}]
                }]}
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let err = run(&api, "S-1", "main", Some(json!([1])), false)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("TypeError: x is undefined") && msg.contains("main (line 4)"),
            "{msg}"
        );
    }

    #[tokio::test]
    async fn run_returns_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scripts/S-1:run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "done": true, "response": {"result": 42}
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert_eq!(
            run(&api, "S-1", "f", None, false).await.unwrap(),
            json!({"result": 42})
        );
    }

    #[test]
    fn args_must_be_array() {
        assert!(parse_args(Some("{}")).is_err());
        assert!(parse_args(Some("nope")).is_err());
        assert_eq!(parse_args(Some("[1]")).unwrap(), Some(json!([1])));
        assert_eq!(parse_args(None).unwrap(), None);
    }

    #[tokio::test]
    async fn pull_writes_files_and_refuses_overwrite() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/S_1/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"name": "appsscript", "type": "JSON", "source": "{}"},
                {"name": "lib/Code", "type": "SERVER_JS", "source": "function a(){}"}
            ]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempdir().unwrap();
        pull(&api, "S_1", dir.path(), false).await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("lib/Code.gs")).unwrap(),
            "function a(){}"
        );
        assert!(pull(&api, "S_1", dir.path(), false).await.is_err());
        pull(&api, "S_1", dir.path(), true).await.unwrap();
    }

    #[tokio::test]
    async fn logs_lists_processes_with_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/processes:listScriptProcesses"))
            .and(query_param("scriptId", "S"))
            .and(query_param("scriptProcessFilter.functionName", "main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "processes": [{"functionName": "main", "processStatus": "COMPLETED"}]
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = logs(&api, "S", Some("main"), Some(10)).await.unwrap();
        assert_eq!(v["count"], 1);
    }

    #[tokio::test]
    async fn push_plans_put_content() {
        let api = dry_api("");
        push(
            &api,
            "S-1",
            vec![json!({"name": "appsscript", "type": "JSON", "source": "{}"})],
        )
        .await
        .unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["method"], "PUT");
        assert!(
            plan[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/v1/projects/S-1/content")
        );
    }
}
