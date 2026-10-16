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
use crate::args::{flag, many, optional, required, required_non_empty};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
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
  Extensions match in any case (Code.GS is pushed as Code).
  Files in sub-directories keep their path (e.g. lib/util.gs -> lib/util).
  Hidden files/directories and node_modules are skipped.
  A symlink to a pushed file or a directory is refused: copy its target in.
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
            let script_id = required_non_empty(m, "script-id")?;
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
///
/// A push replaces every file in the project, so a local file left out is
/// deleted remotely. A symlink that would be pushed, or that points to a
/// directory (which may hold pushable files), is therefore refused rather
/// than skipped: following it could read outside `root`, and skipping it
/// would delete the remote copy of what it stands for.
fn collect_files(root: &Path) -> Result<Vec<Value>, GwsError> {
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    let mut symlinks: Vec<String> = Vec::new();
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
                if symlink_may_be_pushed(root, &path)? {
                    symlinks.push(path.display().to_string());
                } else {
                    tracing::debug!("skipping symlink '{}'", path.display());
                }
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
    if !symlinks.is_empty() {
        symlinks.sort();
        return Err(GwsError::Validation(format!(
            "Refusing to push: symlinks are not followed, and +push replaces every file in \
             the project, so skipping them would delete the matching remote files: {}; \
             replace each with a copy of its target",
            symlinks.join(", ")
        )));
    }
    if files.is_empty() {
        return Err(GwsError::Validation(format!(
            "No .gs/.js/.html/appsscript.json files found in '{}'",
            root.display()
        )));
    }
    // Only `appsscript.json` is the manifest. A root `appsscript.gs` or
    // `appsscript.html` would take its name without being one, and the API
    // rejects the push with an unhelpful error.
    match files.get("appsscript") {
        Some(f) if f["type"] == "JSON" => {}
        Some(f) => {
            return Err(GwsError::Validation(format!(
                "'{}' would be pushed as the Apps Script file 'appsscript', which is reserved for \
                 the appsscript.json manifest; rename it and add appsscript.json (use +pull to \
                 fetch the current one)",
                f["_path"].as_str().unwrap_or("?")
            )));
        }
        None => {
            return Err(GwsError::Validation(format!(
                "'{}' has no appsscript.json manifest; Apps Script requires it (use +pull to fetch the current one)",
                root.display()
            )));
        }
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

/// Whether the symlink at `path` stands for content a push would upload:
/// its own name maps to an Apps Script file, or it points to a directory.
fn symlink_may_be_pushed(root: &Path, path: &Path) -> Result<bool, GwsError> {
    if script_file_for(root, path)?.is_some() {
        return Ok(true);
    }
    match fs::metadata(path) {
        Ok(target) => Ok(target.is_dir()),
        // A dangling link to a name that is not pushed stands for nothing.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(GwsError::Validation(format!(
            "Failed to stat the target of symlink '{}': {e}",
            path.display()
        ))),
    }
}

/// The Apps Script `(type, name)` a local file at `path` is pushed as, or
/// `None` when it is not pushed.
fn script_file_for(root: &Path, path: &Path) -> Result<Option<(&'static str, String)>, GwsError> {
    let rel = path.strip_prefix(root).map_err(http::other_err)?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    // Match the extension case-insensitively: push replaces every file in
    // the project, so skipping `Code.GS` would delete `Code` remotely.
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = rel_str
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| rel_str.clone());
    Ok(match extension.as_str() {
        "gs" | "js" => Some(("SERVER_JS", stem)),
        "html" => Some(("HTML", stem)),
        "json" if stem == "appsscript" => Some(("JSON", stem)),
        _ => None,
    })
}

/// Map one local file to an Apps Script file object, or `None` to skip it.
fn process_file(root: &Path, path: &Path) -> Result<Option<(String, Value)>, GwsError> {
    let Some((type_val, name)) = script_file_for(root, path)? else {
        return Ok(None);
    };
    // A leading UTF-8 byte order mark is an encoding signature, not source.
    let source = fs::read_to_string(path)
        .map(crate::executor::strip_utf8_bom)
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
    let parts: Vec<&str> = name.split('/').collect();
    let last = parts.len() - 1;
    for (i, part) in parts.into_iter().enumerate() {
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
        // The name maps back to the path on +push, so it cannot be renamed
        // on disk; refuse names Windows would treat as a device (`CON.gs`),
        // as a drive-relative path or alternate data stream (`C:x`, `a:b`),
        // or silently change (a trailing dot or space on a directory).
        if crate::helpers::http::is_windows_device_name(part)
            || part.contains(['<', '>', ':', '"', '|', '?', '*'])
            || (i != last && part.ends_with(['.', ' ']))
        {
            return Err(GwsError::Validation(format!(
                "Refusing Apps Script file name '{name}': '{part}' is not a valid file or \
                 directory name on Windows; rename it in the Apps Script editor"
            )));
        }
        // Append the extension rather than `set_extension`, which would
        // replace everything after a dot in the name (`a.b` -> `a.gs`).
        let component = if i == last {
            format!("{part}.{ext}")
        } else {
            part.to_string()
        };
        // +push skips these and then replaces every file in the project, so
        // writing one would make the next push delete it from the project.
        if skip_name(&component) {
            return Err(GwsError::Validation(format!(
                "Refusing Apps Script file name '{name}': +push skips hidden files and \
                 node_modules, so pushing the pulled directory would delete it from the \
                 project; rename it in the Apps Script editor"
            )));
        }
        path.push(component);
    }
    Ok(path)
}

/// Record every prefix of `relative` (each directory and the file itself)
/// under its [`case_insensitive_key`](crate::helpers::http::case_insensitive_key),
/// and refuse two spellings that differ only in case (or Unicode
/// normalization). They are distinct Apps Script names but the same path on
/// case-insensitive disks (macOS, Windows), where one file would silently
/// replace the other, or two directories would merge and `+push` would
/// rename files. The name maps back to the path on `+push`, so the file
/// cannot simply be renamed on disk.
fn claim_case_insensitively(
    claimed: &mut HashMap<String, PathBuf>,
    relative: &Path,
) -> Result<(), GwsError> {
    let mut prefix = PathBuf::new();
    for component in relative.components() {
        prefix.push(component);
        let key = crate::helpers::http::case_insensitive_key(&prefix.to_string_lossy());
        match claimed.get(&key) {
            Some(existing) if *existing != prefix => {
                return Err(GwsError::Validation(format!(
                    "Apps Script files '{}' and '{}' differ only in case (or Unicode \
                     normalization) and would overwrite each other on a case-insensitive disk; rename one in the \
                     Apps Script editor (nothing was written)",
                    existing.display(),
                    prefix.display()
                )));
            }
            Some(_) => {}
            None => {
                claimed.insert(key, prefix.clone());
            }
        }
    }
    Ok(())
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
    // Case-folded path prefix -> the spelling that claimed it first.
    let mut claimed: HashMap<String, PathBuf> = HashMap::new();
    for f in files {
        let name = f.get("name").and_then(Value::as_str).unwrap_or("");
        let file_type = f.get("type").and_then(Value::as_str).unwrap_or("");
        // Proto3 JSON omits empty strings, so a missing `source` is an empty
        // file. Any other non-string value is a response we don't
        // understand: writing it as empty would let the next +push (which
        // replaces the whole project) wipe the file.
        let source = match f.get("source") {
            None => "",
            Some(Value::String(s)) => s.as_str(),
            Some(other) => {
                return Err(GwsError::Validation(format!(
                    "Apps Script file '{name}' has a non-string source ({}); refusing to pull \
                     (nothing was written)",
                    match other {
                        Value::Null => "null",
                        Value::Bool(_) => "boolean",
                        Value::Number(_) => "number",
                        Value::Array(_) => "array",
                        Value::Object(_) => "object",
                        Value::String(_) => "string",
                    }
                )));
            }
        };
        let relative = local_path_for(name, file_type)?;
        claim_case_insensitively(&mut claimed, &relative)?;
        let path = dir.join(relative);
        if !overwrite && path.exists() {
            return Err(GwsError::Validation(format!(
                "'{}' already exists; pass --overwrite to replace local files (nothing was written)",
                path.display()
            )));
        }
        planned.push((path, source));
    }
    let total = planned.len();
    let mut written: Vec<String> = Vec::new();
    for (path, source) in planned {
        if let Err(e) = crate::output_file::write_atomic(&path, source.as_bytes(), overwrite).await
        {
            // The files before this one are already on disk: say which.
            let note = if written.is_empty() {
                format!("Wrote 0 of {total} files")
            } else {
                format!(
                    "Wrote {} of {total} files before failing ({})",
                    written.len(),
                    written.join(", ")
                )
            };
            return Err(crate::helpers::modelarmor::prefix_message(e, &note));
        }
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
    // The error is a `google.rpc.Status`: its code decides the class. A
    // script exception is code 3 (INVALID_ARGUMENT, HTTP 400), permanent;
    // a fixed 500 made every failed run look retryable (exit 6).
    let (code, _) = crate::executor::operation::status_code(details);
    GwsError::Api {
        code,
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

    /// A UTF-8 byte order mark (Notepad, PowerShell) is an encoding
    /// signature, not source: it is not pushed into the manifest JSON or the
    /// code, so a pushed file matches what `+pull` writes back.
    #[test]
    fn collect_files_drops_a_utf8_bom() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("appsscript.json"),
            "\u{feff}{\"timeZone\": \"Etc/UTC\"}",
        );
        write(&dir.path().join("Code.gs"), "\u{feff}function a(){}\r\n");
        write(&dir.path().join("index.html"), "<p>\u{feff}</p>");
        let files = collect_files(dir.path()).unwrap();
        let source = |name: &str| {
            files.iter().find(|f| f["name"] == name).unwrap()["source"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(source("appsscript"), "{\"timeZone\": \"Etc/UTC\"}");
        assert_eq!(source("Code"), "function a(){}\r\n");
        // Only a leading mark is dropped.
        assert_eq!(source("index"), "<p>\u{feff}</p>");
    }

    #[test]
    fn collect_files_matches_extensions_case_insensitively() {
        // push replaces the whole project, so a skipped file is deleted
        // remotely: upper-case extensions must be pushed, not dropped.
        let dir = tempdir().unwrap();
        write(&dir.path().join("appsscript.JSON"), "{}");
        write(&dir.path().join("Code.GS"), "function a(){}");
        write(&dir.path().join("lib/Util.Js"), "function b(){}");
        write(&dir.path().join("Index.HTML"), "<p/>");
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
                ("Index".into(), "HTML".into()),
                ("appsscript".into(), "JSON".into()),
                ("lib/Util".into(), "SERVER_JS".into()),
            ]
        );
    }

    /// Push replaces every file in the project, so a symlink that is
    /// skipped deletes the remote file it stands for. A symlink whose name
    /// would be pushed, or that points to a directory, is refused and named;
    /// one that would not be pushed anyway is still ignored.
    #[cfg(unix)]
    #[test]
    fn collect_files_refuses_symlinks_that_would_be_pushed() {
        use std::os::unix::fs::symlink;
        let shared = tempdir().unwrap();
        write(&shared.path().join("Shared.gs"), "function s(){}");
        write(&shared.path().join("lib/util.gs"), "function u(){}");
        write(&shared.path().join("NOTES.md"), "x");

        let dir = tempdir().unwrap();
        write(&dir.path().join("appsscript.json"), "{}");
        write(&dir.path().join("Code.gs"), "function a(){}");
        // Not pushed either way: ignored, as a regular README.md would be.
        symlink(shared.path().join("NOTES.md"), dir.path().join("README.md")).unwrap();
        symlink(
            shared.path().join("missing"),
            dir.path().join("dangling.txt"),
        )
        .unwrap();
        assert_eq!(collect_files(dir.path()).unwrap().len(), 2);

        symlink(
            shared.path().join("Shared.gs"),
            dir.path().join("Shared.gs"),
        )
        .unwrap();
        symlink(shared.path().join("lib"), dir.path().join("lib")).unwrap();
        let err = collect_files(dir.path()).unwrap_err().to_string();
        assert!(err.contains("Refusing to push"), "{err}");
        assert!(err.contains("Shared.gs"), "{err}");
        assert!(
            err.contains(&dir.path().join("lib").display().to_string()),
            "{err}"
        );
        assert!(!err.contains("README.md"), "{err}");

        // A dangling link with a pushable name is refused too.
        fs::remove_file(dir.path().join("Shared.gs")).unwrap();
        fs::remove_file(dir.path().join("lib")).unwrap();
        symlink(shared.path().join("gone.gs"), dir.path().join("Gone.gs")).unwrap();
        let err = collect_files(dir.path()).unwrap_err().to_string();
        assert!(err.contains("Gone.gs"), "{err}");
    }

    #[test]
    fn collect_files_refuses_names_differing_only_in_extension_case() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("appsscript.json"), "{}");
        write(&dir.path().join("Code.gs"), "x");
        write(&dir.path().join("Code.GS"), "y");
        // Both spellings are one file on a case-insensitive disk.
        if fs::read_dir(dir.path()).unwrap().count() == 3 {
            let err = collect_files(dir.path()).unwrap_err().to_string();
            assert!(err.contains("Two local files map"), "{err}");
        }
    }

    #[test]
    fn collect_files_only_takes_appsscript_json_as_the_manifest() {
        for other in ["appsscript.gs", "appsscript.js", "appsscript.html"] {
            let dir = tempdir().unwrap();
            write(&dir.path().join("Code.gs"), "x");
            write(&dir.path().join(other), "x");
            let err = collect_files(dir.path()).unwrap_err().to_string();
            assert!(
                err.contains(other) && err.contains("appsscript.json"),
                "{other}: {err}"
            );
        }
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
        assert_eq!(
            local_path_for("v1.2/page.min", "HTML").unwrap(),
            PathBuf::from("v1.2/page.min.html")
        );
        assert!(local_path_for("../evil", "HTML").is_err());
        assert!(local_path_for("/abs", "HTML").is_err());
        assert!(local_path_for("x", "WEIRD").is_err());
    }

    /// `+push` skips hidden files and `node_modules` and then replaces every
    /// file in the project, so a pulled file it would skip is deleted from
    /// the project by the next push. `+pull` refuses such names up front.
    #[test]
    fn local_paths_refuse_names_push_would_skip() {
        for name in [".hidden", "lib/.x", ".config/settings", "node_modules/dep"] {
            let err = local_path_for(name, "SERVER_JS").unwrap_err().to_string();
            assert!(err.contains("+push"), "{name}: {err}");
        }
        // Everything +pull writes, +push reads back under the same name.
        let dir = tempdir().unwrap();
        let names = [
            "Code",
            "lib/util",
            "node_modules_old/x",
            "a.b",
            "appsscript",
        ];
        for name in names {
            let ty = if name == "appsscript" {
                "JSON"
            } else {
                "SERVER_JS"
            };
            write(&dir.path().join(local_path_for(name, ty).unwrap()), "x");
        }
        let mut pushed: Vec<String> = collect_files(dir.path())
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect();
        pushed.sort();
        let mut expected: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        expected.sort();
        assert_eq!(pushed, expected);
    }

    #[test]
    fn local_paths_refuse_windows_device_names() {
        // `CON.gs` would open the console on Windows; the file cannot be
        // renamed on disk because +push maps the path back to the name.
        for name in [
            "CON", "con", "lib/nul", "aux/util", "COM1", "lpt9.x", "dir./a", "dir /a",
        ] {
            let err = local_path_for(name, "SERVER_JS").unwrap_err().to_string();
            assert!(err.contains("Windows"), "{name}: {err}");
        }
        for name in [
            "C:evil",
            "lib/D:x",
            "a:b",
            "q?",
            "a*b",
            "x|y",
            "<t>",
            "say\"hi\"",
        ] {
            let err = local_path_for(name, "SERVER_JS").unwrap_err().to_string();
            assert!(err.contains("Windows"), "{name}: {err}");
        }
        assert!(local_path_for("CONTRACT", "SERVER_JS").is_ok());
        assert!(local_path_for("lib/console", "SERVER_JS").is_ok());
    }

    /// On Windows a component such as `C:evil` is a drive-relative path:
    /// `PathBuf::push`/`join` replaces everything before it, so the pulled
    /// file would land in drive C's current directory instead of `--dir`.
    #[test]
    fn local_paths_never_escape_the_directory() {
        let dir = PathBuf::from("pulled");
        for name in ["C:evil", "lib/D:x", "a:b", "z:"] {
            if let Ok(relative) = local_path_for(name, "SERVER_JS") {
                let joined = dir.join(&relative);
                assert!(
                    joined.starts_with(&dir),
                    "{name} -> {} escapes {}",
                    joined.display(),
                    dir.display()
                );
                assert!(
                    relative
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_))),
                    "{name} -> {}",
                    relative.display()
                );
            }
        }
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
        // An exception the script threw is permanent: rerunning the same
        // call fails the same way, so it must not exit 6 (retryable).
        assert!(
            matches!(&err, GwsError::Api { code: 400, reason, .. } if reason == "scriptError"),
            "{err:?}"
        );
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API, "{err:?}");
    }

    /// The `google.rpc.Code` of the operation error decides the class: a
    /// script that ran out of time (DEADLINE_EXCEEDED) may succeed on a
    /// retry.
    #[test]
    fn script_error_status_follows_the_rpc_code() {
        for (code, http, exit) in [
            (3, 400, GwsError::EXIT_CODE_API),
            (4, 504, GwsError::EXIT_CODE_API_RETRYABLE),
            (7, 403, GwsError::EXIT_CODE_API),
        ] {
            let err = script_error(&json!({"code": code, "message": "m"}));
            assert!(
                matches!(&err, GwsError::Api { code: c, .. } if *c == http),
                "{code}: {err:?}"
            );
            assert_eq!(err.exit_code(), exit, "{code}: {err:?}");
        }
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

    /// A write that fails part-way leaves the files before it on disk; the
    /// error must list them so the caller knows the directory changed.
    #[tokio::test]
    async fn pull_failure_part_way_lists_the_files_already_written() {
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
        // A regular file where the `lib` directory must go: the second
        // write fails after the first succeeded.
        fs::write(dir.path().join("lib"), "not a directory").unwrap();
        let err = pull(&api, "S_1", dir.path(), false).await.unwrap_err();
        assert!(dir.path().join("appsscript.json").exists());
        let msg = err.to_string();
        assert!(
            msg.contains(&dir.path().join("appsscript.json").display().to_string()),
            "the error does not list the file already written: {msg}"
        );
        assert!(msg.contains("1 of 2"), "{msg}");
    }

    /// `Code` and `code` (or `lib/a` and `Lib/b`) are distinct in the
    /// API but the same path on case-insensitive disks (macOS, Windows):
    /// one file would silently replace the other. Refuse before writing.
    #[tokio::test]
    async fn pull_refuses_names_differing_only_in_case() {
        for (a, b) in [
            ("Code", "code"),
            ("lib/a", "Lib/b"),
            // Final and medial sigma: one name on APFS and NTFS, though
            // `to_lowercase` keeps them apart.
            ("\u{3b1}\u{3c2}", "\u{3b1}\u{3c3}"),
            ("caf\u{e9}", "cafe\u{301}"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/projects/S_1/content"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                    {"name": a, "type": "SERVER_JS", "source": "var first;"},
                    {"name": b, "type": "SERVER_JS", "source": "var second;"}
                ]})))
                .mount(&server)
                .await;
            let api = api(&server.uri(), "");
            let dir = tempdir().unwrap();
            let err = pull(&api, "S_1", dir.path(), true).await.unwrap_err();
            assert!(matches!(err, GwsError::Validation(_)), "{a}/{b}: {err:?}");
            assert!(err.to_string().contains("differ only in case"), "{err}");
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                0,
                "nothing written"
            );
        }
    }

    /// Apps Script file names may contain dots. `a.b` and `a.c` are two
    /// distinct server files and must land in two local files that `+push`
    /// maps back to the same names, not both in `a.gs`.
    #[tokio::test]
    async fn pull_keeps_dots_in_file_names() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/S_1/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"name": "appsscript", "type": "JSON", "source": "{}"},
                {"name": "config.dev", "type": "SERVER_JS", "source": "var env = 'dev';"},
                {"name": "config.prod", "type": "SERVER_JS", "source": "var env = 'prod';"}
            ]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempdir().unwrap();
        pull(&api, "S_1", dir.path(), true).await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("config.dev.gs")).unwrap(),
            "var env = 'dev';"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("config.prod.gs")).unwrap(),
            "var env = 'prod';"
        );
        // The pulled directory pushes back under the original names.
        let mut names: Vec<String> = collect_files(dir.path())
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["appsscript", "config.dev", "config.prod"]);
    }

    /// A `source` that is present but not a string is a response we do not
    /// understand. Writing it as an empty file would let the next `+push`
    /// (which replaces the whole project) wipe that file, so refuse before
    /// writing anything and name the file.
    #[tokio::test]
    async fn pull_refuses_non_string_source() {
        for bad in [json!(null), json!(42), json!({"text": "x"}), json!(["x"])] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/projects/S_1/content"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                    {"name": "appsscript", "type": "JSON", "source": "{}"},
                    {"name": "Code", "type": "SERVER_JS", "source": bad}
                ]})))
                .mount(&server)
                .await;
            let api = api(&server.uri(), "");
            let dir = tempdir().unwrap();
            let err = pull(&api, "S_1", dir.path(), true).await.unwrap_err();
            assert!(matches!(err, GwsError::Validation(_)), "{bad}: {err:?}");
            assert!(err.to_string().contains("'Code'"), "{err}");
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                0,
                "nothing written for {bad}"
            );
        }
    }

    /// Proto3 JSON omits empty strings, so an empty project file arrives
    /// without `source`. It is pulled as an empty file and pushes back as
    /// the same empty file.
    #[tokio::test]
    async fn pull_writes_omitted_source_as_empty_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/projects/S_1/content"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"name": "appsscript", "type": "JSON", "source": "{}"},
                {"name": "Empty", "type": "SERVER_JS"}
            ]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let dir = tempdir().unwrap();
        pull(&api, "S_1", dir.path(), true).await.unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("Empty.gs")).unwrap(), "");
        let pushed = collect_files(dir.path()).unwrap();
        let empty = pushed.iter().find(|f| f["name"] == "Empty").unwrap();
        assert_eq!(empty["source"], "");
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
