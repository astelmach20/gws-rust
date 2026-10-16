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

//! Drive helpers: `+upload`, `+download`, `+export`, `+move`, `+share`, `+sync`.
//!
//! Every call sets `supportsAllDrives=true` so Shared Drive items work.

use super::Helper;
use super::http::{self, Api, ApiRequest, OutputTarget, case_insensitive_key, safe_filename};
use crate::args::{flag, optional, required};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

const SCOPE_DRIVE: &str = "https://www.googleapis.com/auth/drive";
const SCOPE_DRIVE_READONLY: &str = "https://www.googleapis.com/auth/drive.readonly";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const FILE_FIELDS: &str =
    "id,name,mimeType,parents,size,modifiedTime,webViewLink,driveId,shortcutDetails";

pub struct DriveHelper;

fn file_id_arg() -> Arg {
    Arg::new("file-id")
        .long("file-id")
        .help("Drive file ID")
        .required(true)
        .value_name("ID")
}

fn output_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("output")
            .long("output")
            .short('o')
            .help("Local path to write, or '-' for stdout. Defaults to the Drive file name")
            .value_name("PATH"),
    )
    .arg(
        Arg::new("overwrite")
            .long("overwrite")
            .help("Replace an existing local file")
            .action(ArgAction::SetTrue),
    )
}

impl Helper for DriveHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+upload")
                .about("[Helper] Upload a local file (resumable, Shared Drive aware)")
                .arg(
                    Arg::new("file")
                        .long("file")
                        .help("Local file to upload")
                        .required(true)
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("folder-id")
                        .long("folder-id")
                        .help("Destination folder ID (My Drive or Shared Drive). Defaults to My Drive root")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .allow_hyphen_values(true)
                        .help("Name in Drive (defaults to the local file name)")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("mime-type")
                        .long("mime-type")
                        .help("Content type of the local file (default: detected from the extension)")
                        .value_name("TYPE"),
                )
                .arg(
                    Arg::new("convert")
                        .long("convert")
                        .help("Convert to the matching Google format (Docs/Sheets/Slides) on import")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +upload --file ./report.pdf
  gwsr drive +upload --file ./report.pdf --folder-id FOLDER_ID
  gwsr drive +upload --file ./data.csv --name 'Sales Data' --convert

TIPS:
  Uses the resumable upload protocol in 8 MiB chunks, so large files work.
  Works with Shared Drive folders (supportsAllDrives is always set).
  --convert turns .docx/.csv/.xlsx/.pptx/... into native Google files.",
                ),
        )
        .subcommand(output_args(
            Command::new("+download")
                .about("[Helper] Download a (non-Google-native) file's content")
                .arg(file_id_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +download --file-id FILE_ID
  gwsr drive +download --file-id FILE_ID --output ./copy.pdf
  gwsr drive +download --file-id FILE_ID --output - | sha256sum

TIPS:
  Google Docs/Sheets/Slides have no binary content; use +export instead.
  Existing local files are never replaced unless --overwrite is given.",
                ),
        ))
        .subcommand(output_args(
            Command::new("+export")
                .about("[Helper] Export a Google Doc/Sheet/Slides/Drawing to another format")
                .arg(file_id_arg())
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Target format")
                        .required(true)
                        .value_parser(EXPORT_FORMATS.iter().map(|f| f.name).collect::<Vec<_>>())
                        .value_name("FORMAT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +export --file-id DOC_ID --to pdf
  gwsr drive +export --file-id DOC_ID --to md --output -
  gwsr drive +export --file-id SHEET_ID --to csv --output sheet1.csv

TIPS:
  Docs: pdf docx odt rtf txt md html epub
  Sheets: pdf xlsx ods csv tsv html (csv/tsv export the first sheet)
  Slides: pdf pptx odp txt
  Drawings: pdf png jpg svg
  The Drive export endpoint is limited to 10 MB of output.",
                ),
        ))
        .subcommand(
            Command::new("+move")
                .about("[Helper] Move a file or folder to another folder")
                .arg(file_id_arg())
                .arg(
                    Arg::new("folder-id")
                        .long("folder-id")
                        .help("Destination folder ID")
                        .required(true)
                        .value_name("ID"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +move --file-id FILE_ID --folder-id FOLDER_ID

TIPS:
  Removes all current parents and adds the destination folder.",
                ),
        )
        .subcommand(with_yes(
            Command::new("+share")
                .about("[Helper] Grant access to a file or folder")
                .arg(file_id_arg())
                .arg(
                    Arg::new("role")
                        .long("role")
                        .help("Role to grant (required; there is no default)")
                        .required(true)
                        .value_parser([
                            "reader",
                            "commenter",
                            "writer",
                            "fileOrganizer",
                            "organizer",
                            "owner",
                        ])
                        .value_name("ROLE"),
                )
                .arg(
                    Arg::new("email")
                        .long("email")
                        .help("Grant to this user (or group, with --group)")
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("group")
                        .long("group")
                        .help("Treat --email as a Google Group address")
                        .requires("email")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("domain")
                        .long("domain")
                        .help("Grant to everyone in this domain")
                        .value_name("DOMAIN"),
                )
                .arg(
                    Arg::new("anyone")
                        .long("anyone")
                        .help("Grant to anyone with the link")
                        .action(ArgAction::SetTrue),
                )
                .group(
                    clap::ArgGroup::new("grantee")
                        .args(["email", "domain", "anyone"])
                        .required(true),
                )
                .arg(
                    Arg::new("no-notify")
                        .long("no-notify")
                        .help("Do not send a notification email")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("message")
                        .long("message")
                        .allow_hyphen_values(true)
                        .help("Custom message for the notification email")
                        .conflicts_with("no-notify")
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +share --file-id FILE_ID --email bob@example.com --role reader
  gwsr drive +share --file-id FILE_ID --email team@example.com --group --role writer
  gwsr drive +share --file-id FILE_ID --domain example.com --role commenter
  gwsr drive +share --file-id FILE_ID --email bob@example.com --role owner --yes

TIPS:
  --role is mandatory so access is never granted implicitly.
  Transferring ownership, or granting writer/organizer to a domain or to
  anyone with the link, always requires confirmation (--yes, or a prompt on
  a terminal). Other grants require it when GWSR_REQUIRE_CONFIRM=1.",
                ),
        ))
        .subcommand(
            Command::new("+sync")
                .about("[Helper] Mirror a Drive folder into a local directory (one-way, download only)")
                .arg(
                    Arg::new("folder-id")
                        .long("folder-id")
                        .help("Drive folder to mirror")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("dir")
                        .long("dir")
                        .help("Local directory to mirror into (created if missing)")
                        .required(true)
                        .value_name("DIR"),
                )
                .arg(
                    Arg::new("overwrite")
                        .long("overwrite")
                        .help("Also replace local files that changed since the last sync, or that it did not write (default: keep them and report them under \"conflicts\")")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror
  gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --dry-run
  gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --overwrite

TIPS:
  Recurses into sub-folders. Regular files are downloaded; Google
  Docs/Sheets/Slides/Drawings are exported as docx/xlsx/pptx/pdf.
  What was mirrored is recorded in DIR/.gwsr-sync.json, keyed by Drive file
  ID (Drive version, checksums, modified time; local size, mtime, SHA-256).
  A file is downloaded again only when Drive's copy changed, or another
  Drive item now has its name. A local file that was edited since the last
  sync, or that the sync did not write, is never overwritten without
  --overwrite: it is listed under \"conflicts\" (reason modifiedLocally or
  untracked) and the command exits 1 with reason syncConflict after printing
  the report. A missing or corrupt state file makes every existing local
  file untracked, except files whose Drive SHA-256 checksum matches. Local
  files that are not in Drive are left untouched (nothing is ever deleted).
  Other Google-native types (Forms, Sites, shortcuts...) are reported as skipped.
  A file the API refuses (download restricted, export too large) is listed
  under \"failed\" and the rest are still mirrored; the report is printed and
  the command then exits 1 with reason syncPartialFailure.",
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
            let dry = crate::args::dry_run(m)?;
            let result = match name {
                "+upload" => {
                    let args = UploadArgs::parse(m)?;
                    let api = Api::new(doc, &[SCOPE_DRIVE], dry, sanitize).await?;
                    let v = upload(&api, &args).await?;
                    (api, v)
                }
                "+download" => {
                    check_output(m)?;
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let v = download(
                        &api,
                        required(m, "file-id")?,
                        optional(m, "output")?,
                        flag(m, "overwrite")?,
                    )
                    .await?;
                    if streamed_to_stdout(&api, m)? {
                        return Ok(true);
                    }
                    (api, v)
                }
                "+export" => {
                    check_output(m)?;
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let v = export(
                        &api,
                        required(m, "file-id")?,
                        required(m, "to")?,
                        optional(m, "output")?,
                        flag(m, "overwrite")?,
                    )
                    .await?;
                    if streamed_to_stdout(&api, m)? {
                        return Ok(true);
                    }
                    (api, v)
                }
                "+move" => {
                    let api = Api::new(doc, &[SCOPE_DRIVE], dry, sanitize).await?;
                    let v =
                        move_file(&api, required(m, "file-id")?, required(m, "folder-id")?).await?;
                    (api, v)
                }
                "+share" => {
                    let args = ShareArgs::parse(m)?;
                    let (impact, action) = args.impact();
                    confirm::confirm(m, impact, &action)?;
                    let api = Api::new(doc, &[SCOPE_DRIVE], dry, sanitize).await?;
                    let v = share(&api, &args).await?;
                    (api, v)
                }
                "+sync" => {
                    let dir = crate::validate::validate_safe_output_dir(required(m, "dir")?)?;
                    let overwrite = flag(m, "overwrite")?;
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let folder_id = required(m, "folder-id")?;
                    let v = match run_sync(&api, folder_id, &dir, overwrite).await? {
                        SyncEnd::Done(v) => v,
                        SyncEnd::Interrupted(signal) => {
                            tracing::warn!(
                                "drive +sync interrupted by {}; the sync state of the files finished so far was saved to '{}'; run it again to continue",
                                signal.name(),
                                dir.join(SYNC_STATE_FILE).display()
                            );
                            crate::helpers::exit_for_signal(signal);
                            return Ok(true);
                        }
                    };
                    api.emit(m, &v).await?;
                    return match sync_failure(&v) {
                        Some(err) => Err(err),
                        None => Ok(true),
                    };
                }
                _ => return Ok(false),
            };
            let (api, value) = result;
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

// ── +upload ──────────────────────────────────────────────────────────

struct UploadArgs {
    path: PathBuf,
    name: String,
    folder_id: Option<String>,
    content_type: String,
    convert: bool,
}

impl UploadArgs {
    fn parse(m: &ArgMatches) -> Result<Self, GwsError> {
        let raw = required(m, "file")?;
        let path = crate::validate::validate_safe_file_path(raw, "--file")?;
        if !path.is_file() {
            return Err(GwsError::Validation(format!(
                "--file '{raw}' is not a readable file"
            )));
        }
        let name = match optional(m, "name")? {
            Some(n) => n.to_string(),
            None => determine_filename(raw)?,
        };
        let content_type = match optional(m, "mime-type")? {
            Some(t) => {
                check_mime_type(t)?;
                t.to_string()
            }
            None => mime_guess2::from_path(&path)
                .first()
                .map(|m| m.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string()),
        };
        Ok(Self {
            path,
            name,
            folder_id: optional(m, "folder-id")?.map(str::to_string),
            content_type,
            convert: flag(m, "convert")?,
        })
    }
}

/// Validate `--mime-type`, which is sent as the `X-Upload-Content-Type`
/// header: no control characters (which would otherwise fail only when the
/// request is built, as a network error) and a `type/subtype` essence.
fn check_mime_type(value: &str) -> Result<(), GwsError> {
    crate::validate::reject_dangerous_chars(value, "--mime-type")?;
    let essence = value.split(';').next().unwrap_or_default().trim();
    let valid_token = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
    };
    match essence.split_once('/') {
        Some((kind, sub)) if valid_token(kind) && valid_token(sub) => Ok(()),
        _ => Err(GwsError::Validation(format!(
            "--mime-type '{value}' is not a media type (expected type/subtype, e.g. text/csv)"
        ))),
    }
}

fn determine_filename(file_path: &str) -> Result<String, GwsError> {
    Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            GwsError::Validation(format!(
                "Cannot derive a file name from '{file_path}'; pass --name"
            ))
        })
}

/// Google-native type to convert an uploaded file into, by source MIME type
/// (the Docs, Sheets and Slides import formats Drive lists in
/// `about.importFormats`). A media type is case-insensitive and its
/// parameters (`; charset=utf-8`) do not change the format, so only the
/// lowercased essence is compared: `mime_guess2` spells the macro-enabled
/// Office types `macroEnabled`, Drive `macroenabled`.
fn conversion_target(content_type: &str) -> Option<&'static str> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    Some(match essence.as_str() {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.template"
        | "application/vnd.ms-word.document.macroenabled.12"
        | "application/vnd.ms-word.template.macroenabled.12"
        | "application/msword"
        | "application/vnd.oasis.opendocument.text"
        | "application/x-vnd.oasis.opendocument.text"
        | "application/rtf"
        | "text/rtf"
        | "text/richtext"
        | "text/plain"
        | "text/markdown"
        | "text/x-markdown"
        | "text/html" => "application/vnd.google-apps.document",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.template"
        | "application/vnd.ms-excel.sheet.macroenabled.12"
        | "application/vnd.ms-excel.template.macroenabled.12"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet"
        | "application/x-vnd.oasis.opendocument.spreadsheet"
        | "text/csv"
        | "text/tab-separated-values" => "application/vnd.google-apps.spreadsheet",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        | "application/vnd.openxmlformats-officedocument.presentationml.template"
        | "application/vnd.openxmlformats-officedocument.presentationml.slideshow"
        | "application/vnd.ms-powerpoint.presentation.macroenabled.12"
        | "application/vnd.ms-powerpoint.template.macroenabled.12"
        | "application/vnd.ms-powerpoint.slideshow.macroenabled.12"
        | "application/vnd.ms-powerpoint"
        | "application/vnd.oasis.opendocument.presentation"
        | "application/x-vnd.oasis.opendocument.presentation" => {
            "application/vnd.google-apps.presentation"
        }
        _ => return None,
    })
}

fn build_metadata(args: &UploadArgs) -> Result<Value, GwsError> {
    let mut metadata = json!({ "name": args.name });
    if let Some(parent) = &args.folder_id {
        metadata["parents"] = json!([parent]);
    }
    if args.convert {
        let target = conversion_target(&args.content_type).ok_or_else(|| {
            GwsError::Validation(format!(
                "--convert: no Google format accepts '{}' (pass --mime-type if detection was wrong)",
                args.content_type
            ))
        })?;
        metadata["mimeType"] = json!(target);
    }
    Ok(metadata)
}

async fn upload(api: &Api, args: &UploadArgs) -> Result<Value, GwsError> {
    let init = ApiRequest::post(api.root_url("upload/drive/v3/files"))
        .query("uploadType", "resumable")
        .query("supportsAllDrives", "true")
        .query("fields", FILE_FIELDS)
        .json(build_metadata(args)?);
    api.upload_resumable(init, &args.path, &args.content_type)
        .await
}

// ── metadata / download / export ─────────────────────────────────────

async fn get_metadata(api: &Api, file_id: &str) -> Result<Value, GwsError> {
    api.send(
        ApiRequest::get(api.url(&format!("files/{}", encode_path_segment(file_id))))
            .query("supportsAllDrives", "true")
            .query("fields", FILE_FIELDS),
    )
    .await
}

fn str_field<'v>(v: &'v Value, key: &str) -> Result<&'v str, GwsError> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| http::other_err(anyhow::anyhow!("Drive response is missing '{key}'")))
}

/// Check an explicit `--output` before credentials are loaded, so a path the
/// download could never write to is a validation error, not an auth error.
/// Without `--output` the default file name comes from Drive at run time.
fn check_output(m: &ArgMatches) -> Result<(), GwsError> {
    if let Some(o) = optional(m, "output")? {
        OutputTarget::parse(o, flag(m, "overwrite")?)?;
    }
    Ok(())
}

/// `--output`, or a local file named after the Drive file. `ext` (for an
/// export) is appended after the name is made safe, so shortening a long
/// name never cuts the extension off.
fn resolve_target(
    output: Option<&str>,
    default_name: &str,
    ext: Option<&str>,
    overwrite: bool,
) -> Result<OutputTarget, GwsError> {
    if let Some(o) = output {
        return OutputTarget::parse(o, overwrite);
    }
    let safe = safe_filename(default_name, "download");
    let name = match ext {
        Some(ext) => with_extension(&safe, ext),
        None => safe,
    };
    // A name taken from Drive is always a file, never `-` (stdout): only
    // the user's own `--output -` may send raw bytes to stdout.
    let path = crate::validate::validate_output_file_path(&name, "--output")?;
    Ok(OutputTarget::File { path, overwrite })
}

/// Whether `--output -` sent the payload to stdout, which must then carry
/// nothing else: no JSON summary after the raw bytes. A dry run still
/// prints its plan.
fn streamed_to_stdout(api: &Api, m: &ArgMatches) -> Result<bool, GwsError> {
    Ok(!api.is_dry_run() && optional(m, "output")? == Some("-"))
}

fn download_result(file_id: &str, target: &OutputTarget, bytes: Option<u64>) -> Value {
    json!({ "id": file_id, "output": target.describe(), "bytes": bytes })
}

async fn download(
    api: &Api,
    file_id: &str,
    output: Option<&str>,
    overwrite: bool,
) -> Result<Value, GwsError> {
    let (name, target) = if api.is_dry_run() {
        (
            "<file name>".to_string(),
            resolve_target(output, "download", None, overwrite)?,
        )
    } else {
        let meta = get_metadata(api, file_id).await?;
        let mime = str_field(&meta, "mimeType")?;
        if mime.starts_with("application/vnd.google-apps.") {
            return Err(native_download_error(&meta, file_id, mime)?);
        }
        let name = str_field(&meta, "name")?.to_string();
        let target = resolve_target(output, &name, None, overwrite)?;
        (name, target)
    };
    let bytes = api
        .download(
            ApiRequest::get(api.url(&format!("files/{}", encode_path_segment(file_id))))
                .query("alt", "media")
                .query("supportsAllDrives", "true"),
            &target,
        )
        .await?;
    let mut out = download_result(file_id, &target, bytes);
    out["name"] = json!(name);
    Ok(out)
}

const SHORTCUT: &str = "application/vnd.google-apps.shortcut";

/// The error for `+download` of a Google-native file (`meta` has `mimeType`
/// `mime`). It points only at a command that can work: `+export` with the
/// formats valid for the type, the shortcut's target, or nothing.
fn native_download_error(meta: &Value, file_id: &str, mime: &str) -> Result<GwsError, GwsError> {
    let name = str_field(meta, "name")?;
    if mime == SHORTCUT {
        let target = shortcut_target(meta, file_id)?;
        return Ok(GwsError::Validation(format!(
            "'{name}' is a shortcut; download its target with `gwsr drive +download --file-id {target}`"
        )));
    }
    let formats: Vec<&str> = EXPORT_FORMATS
        .iter()
        .filter(|f| f.sources.contains(&mime))
        .map(|f| f.name)
        .collect();
    Ok(GwsError::Validation(if formats.is_empty() {
        format!("'{name}' is a Google-native file ({mime}) that cannot be downloaded or exported")
    } else {
        format!(
            "'{name}' is a Google-native file ({mime}) with no binary content; use `gwsr drive +export --file-id {file_id} --to <format>` (formats: {})",
            formats.join(", ")
        )
    }))
}

/// The ID of the file a shortcut (`meta`, the metadata of `file_id`) points at.
fn shortcut_target<'v>(meta: &'v Value, file_id: &str) -> Result<&'v str, GwsError> {
    meta.get("shortcutDetails")
        .and_then(|d| d.get("targetId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            http::other_err(anyhow::anyhow!(
                "Drive response for shortcut '{file_id}' is missing 'shortcutDetails.targetId'"
            ))
        })
}

struct ExportFormat {
    name: &'static str,
    mime: &'static str,
    ext: &'static str,
    /// Google-native source types this format can be exported from.
    sources: &'static [&'static str],
}

const DOC: &str = "application/vnd.google-apps.document";
const SHEET: &str = "application/vnd.google-apps.spreadsheet";
const SLIDES: &str = "application/vnd.google-apps.presentation";
const DRAWING: &str = "application/vnd.google-apps.drawing";

const EXPORT_FORMATS: &[ExportFormat] = &[
    ExportFormat {
        name: "pdf",
        mime: "application/pdf",
        ext: "pdf",
        sources: &[DOC, SHEET, SLIDES, DRAWING],
    },
    ExportFormat {
        name: "docx",
        mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ext: "docx",
        sources: &[DOC],
    },
    ExportFormat {
        name: "odt",
        mime: "application/vnd.oasis.opendocument.text",
        ext: "odt",
        sources: &[DOC],
    },
    ExportFormat {
        name: "rtf",
        mime: "application/rtf",
        ext: "rtf",
        sources: &[DOC],
    },
    ExportFormat {
        name: "txt",
        mime: "text/plain",
        ext: "txt",
        sources: &[DOC, SLIDES],
    },
    ExportFormat {
        name: "md",
        mime: "text/markdown",
        ext: "md",
        sources: &[DOC],
    },
    ExportFormat {
        name: "html",
        mime: "application/zip",
        ext: "zip",
        sources: &[DOC, SHEET],
    },
    ExportFormat {
        name: "epub",
        mime: "application/epub+zip",
        ext: "epub",
        sources: &[DOC],
    },
    ExportFormat {
        name: "xlsx",
        mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ext: "xlsx",
        sources: &[SHEET],
    },
    ExportFormat {
        name: "ods",
        mime: "application/vnd.oasis.opendocument.spreadsheet",
        ext: "ods",
        sources: &[SHEET],
    },
    ExportFormat {
        name: "csv",
        mime: "text/csv",
        ext: "csv",
        sources: &[SHEET],
    },
    ExportFormat {
        name: "tsv",
        mime: "text/tab-separated-values",
        ext: "tsv",
        sources: &[SHEET],
    },
    ExportFormat {
        name: "pptx",
        mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ext: "pptx",
        sources: &[SLIDES],
    },
    ExportFormat {
        name: "odp",
        mime: "application/vnd.oasis.opendocument.presentation",
        ext: "odp",
        sources: &[SLIDES],
    },
    ExportFormat {
        name: "png",
        mime: "image/png",
        ext: "png",
        sources: &[DRAWING],
    },
    ExportFormat {
        name: "jpg",
        mime: "image/jpeg",
        ext: "jpg",
        sources: &[DRAWING],
    },
    ExportFormat {
        name: "svg",
        mime: "image/svg+xml",
        ext: "svg",
        sources: &[DRAWING],
    },
];

fn export_format(name: &str) -> Result<&'static ExportFormat, GwsError> {
    EXPORT_FORMATS
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| GwsError::Validation(format!("Unknown export format '{name}'")))
}

/// Check the export format is valid for the file's type.
fn check_export(format: &ExportFormat, source_mime: &str) -> Result<(), GwsError> {
    if format.sources.contains(&source_mime) {
        return Ok(());
    }
    let valid: Vec<&str> = EXPORT_FORMATS
        .iter()
        .filter(|f| f.sources.contains(&source_mime))
        .map(|f| f.name)
        .collect();
    if valid.is_empty() {
        // `+download` refuses every Google-native type, so only a regular
        // file may be sent there.
        return Err(GwsError::Validation(
            if source_mime.starts_with("application/vnd.google-apps.") {
                format!(
                    "Files of type {source_mime} are Google-native files that cannot be downloaded or exported"
                )
            } else {
                format!(
                    "Files of type {source_mime} cannot be exported; use +download for regular files"
                )
            },
        ));
    }
    Err(GwsError::Validation(format!(
        "Cannot export {source_mime} as '{}'; valid formats: {}",
        format.name,
        valid.join(", ")
    )))
}

fn with_extension(name: &str, ext: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with(&format!(".{ext}")) {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

async fn export(
    api: &Api,
    file_id: &str,
    to: &str,
    output: Option<&str>,
    overwrite: bool,
) -> Result<Value, GwsError> {
    let format = export_format(to)?;
    let name = if api.is_dry_run() {
        "export".to_string()
    } else {
        let meta = get_metadata(api, file_id).await?;
        let mime = str_field(&meta, "mimeType")?;
        if mime == SHORTCUT {
            let target = shortcut_target(&meta, file_id)?;
            return Err(GwsError::Validation(format!(
                "'{}' is a shortcut; export its target with `gwsr drive +export --file-id {target} --to {}`",
                str_field(&meta, "name")?,
                format.name
            )));
        }
        check_export(format, mime)?;
        str_field(&meta, "name")?.to_string()
    };
    let target = resolve_target(output, &name, Some(format.ext), overwrite)?;
    let bytes = api
        .download(
            ApiRequest::get(api.url(&format!("files/{}/export", encode_path_segment(file_id))))
                .query("mimeType", format.mime),
            &target,
        )
        .await?;
    let mut out = download_result(file_id, &target, bytes);
    out["name"] = json!(name);
    out["mimeType"] = json!(format.mime);
    Ok(out)
}

// ── +move ────────────────────────────────────────────────────────────

async fn move_file(api: &Api, file_id: &str, folder_id: &str) -> Result<Value, GwsError> {
    let current_parents = if api.is_dry_run() {
        "<current parents>".to_string()
    } else {
        let meta = get_metadata(api, file_id).await?;
        let parents: Vec<&str> = meta
            .get("parents")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if parents.contains(&folder_id) && parents.len() == 1 {
            return Err(GwsError::Validation(format!(
                "File is already in folder {folder_id}"
            )));
        }
        parents
            .into_iter()
            .filter(|p| *p != folder_id)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut req = ApiRequest::patch(api.url(&format!("files/{}", encode_path_segment(file_id))))
        .query("addParents", folder_id)
        .query("supportsAllDrives", "true")
        .query("fields", FILE_FIELDS)
        .json(json!({}));
    if !current_parents.is_empty() {
        req = req.query("removeParents", current_parents);
    }
    api.send(req).await
}

// ── +share ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct ShareArgs {
    file_id: String,
    role: String,
    grantee: Grantee,
    notify: bool,
    message: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Grantee {
    User(String),
    Group(String),
    Domain(String),
    Anyone,
}

impl ShareArgs {
    fn parse(m: &ArgMatches) -> Result<Self, GwsError> {
        let grantee = if let Some(email) = optional(m, "email")? {
            if flag(m, "group")? {
                Grantee::Group(email.to_string())
            } else {
                Grantee::User(email.to_string())
            }
        } else if let Some(domain) = optional(m, "domain")? {
            Grantee::Domain(domain.to_string())
        } else if flag(m, "anyone")? {
            Grantee::Anyone
        } else {
            return Err(GwsError::Validation(
                "one of --email, --domain or --anyone is required".into(),
            ));
        };
        let args = Self {
            file_id: required(m, "file-id")?.to_string(),
            role: required(m, "role")?.to_string(),
            grantee,
            notify: !flag(m, "no-notify")?,
            message: optional(m, "message")?.map(str::to_string),
        };
        args.check()?;
        Ok(args)
    }

    /// Reject combinations Drive would refuse, before the confirmation
    /// prompt and before credentials are loaded.
    fn check(&self) -> Result<(), GwsError> {
        let is_person = matches!(self.grantee, Grantee::User(_) | Grantee::Group(_));
        if self.message.is_some() && !is_person {
            return Err(GwsError::Validation(
                "--message only applies to --email grants".into(),
            ));
        }
        if self.role == "owner" {
            if !matches!(self.grantee, Grantee::User(_)) {
                return Err(GwsError::Validation(
                    "--role owner can only be granted to a single user (--email without --group)"
                        .into(),
                ));
            }
            // permissions.create `sendNotificationEmail`: "It must not be
            // disabled for ownership transfers."
            if !self.notify {
                return Err(GwsError::Validation(
                    "--no-notify cannot be used with --role owner: Drive always notifies the new owner of an ownership transfer"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    fn grantee_label(&self) -> String {
        match &self.grantee {
            Grantee::User(e) => e.clone(),
            Grantee::Group(e) => format!("group {e}"),
            Grantee::Domain(d) => format!("everyone in {d}"),
            Grantee::Anyone => "anyone with the link".into(),
        }
    }

    /// Ownership transfer and edit-level access for a whole domain or the
    /// public are destructive-class; every other grant is outbound.
    fn impact(&self) -> (Impact, String) {
        let broad = matches!(self.grantee, Grantee::Domain(_) | Grantee::Anyone);
        let action = if self.role == "owner" {
            format!(
                "transfer ownership of {} to {}",
                self.file_id,
                self.grantee_label()
            )
        } else {
            format!(
                "grant '{}' on {} to {}",
                self.role,
                self.file_id,
                self.grantee_label()
            )
        };
        let destructive =
            self.role == "owner" || (broad && self.role != "reader" && self.role != "commenter");
        (
            if destructive {
                Impact::Destructive
            } else {
                Impact::Outbound
            },
            action,
        )
    }

    fn body(&self) -> Value {
        let mut body = json!({ "role": self.role });
        match &self.grantee {
            Grantee::User(e) => {
                body["type"] = json!("user");
                body["emailAddress"] = json!(e);
            }
            Grantee::Group(e) => {
                body["type"] = json!("group");
                body["emailAddress"] = json!(e);
            }
            Grantee::Domain(d) => {
                body["type"] = json!("domain");
                body["domain"] = json!(d);
            }
            Grantee::Anyone => body["type"] = json!("anyone"),
        }
        body
    }
}

async fn share(api: &Api, args: &ShareArgs) -> Result<Value, GwsError> {
    let is_person = matches!(args.grantee, Grantee::User(_) | Grantee::Group(_));
    let mut req = ApiRequest::post(api.url(&format!(
        "files/{}/permissions",
        encode_path_segment(&args.file_id)
    )))
    .query("supportsAllDrives", "true")
    .json(args.body());
    if is_person {
        req = req.query("sendNotificationEmail", args.notify.to_string());
        if let Some(msg) = &args.message {
            req = req.query("emailMessage", msg.clone());
        }
    }
    if args.role == "owner" {
        req = req.query("transferOwnership", "true");
    }
    api.send(req).await
}

// ── +sync ────────────────────────────────────────────────────────────

/// Export format used by `+sync` for each exportable Google-native type.
fn sync_export(mime: &str) -> Option<&'static ExportFormat> {
    let name = match mime {
        DOC => "docx",
        SHEET => "xlsx",
        SLIDES => "pptx",
        DRAWING => "pdf",
        _ => return None,
    };
    EXPORT_FORMATS.iter().find(|f| f.name == name)
}

/// NFC form of a name. macOS file systems treat the NFC and NFD spellings
/// of a name as the same file, while Drive keeps them distinct.
fn nfc(name: &str) -> std::borrow::Cow<'_, str> {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(name)
}

async fn list_children(api: &Api, folder_id: &str) -> Result<Vec<Value>, GwsError> {
    if folder_id.contains('\'') || folder_id.contains('\\') {
        return Err(GwsError::Validation(format!(
            "Invalid folder ID '{folder_id}'"
        )));
    }
    let page = api
        .paginate(
            ApiRequest::get(api.url("files"))
                .query("q", format!("'{folder_id}' in parents and trashed = false"))
                .query("supportsAllDrives", "true")
                .query("includeItemsFromAllDrives", "true")
                .query("pageSize", "1000")
                .query(
                    "fields",
                    "nextPageToken,files(id,name,mimeType,modifiedTime,size,version,md5Checksum,sha256Checksum)",
                ),
            "files",
            None,
        )
        .await?;
    Ok(page.items)
}

/// The sync-state file `+sync` keeps in the root of `--dir`. No Drive item
/// maps to it: `safe_filename` strips leading dots from Drive names, and a
/// disambiguated name starts with the item's ID.
const SYNC_STATE_FILE: &str = ".gwsr-sync.json";
/// Lock file `+sync` holds in the root of `--dir` for the whole run, so two
/// runs on one directory cannot overwrite each other's state. Like
/// [`SYNC_STATE_FILE`], no Drive item maps to it.
const SYNC_LOCK_FILE: &str = ".gwsr-sync.lock";
/// Schema version of [`SYNC_STATE_FILE`]. A file with any other version is
/// treated like a corrupt one.
const SYNC_STATE_VERSION: u32 = 1;
/// A state file larger than this is treated as corrupt rather than read.
const SYNC_STATE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// What Drive said about an item's content when it was last mirrored.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoteFingerprint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    md5_checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256_checksum: Option<String>,
}

impl RemoteFingerprint {
    fn of(item: &Value) -> Self {
        // int64 fields (`size`, `version`) are JSON strings in Drive v3.
        let field = |key: &str| match item.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        Self {
            modified_time: field("modifiedTime"),
            version: field("version"),
            size: field("size"),
            md5_checksum: field("md5Checksum"),
            sha256_checksum: field("sha256Checksum"),
        }
    }

    /// Whether the item now (`self`) has the content recorded in `then`.
    /// Checksums decide when both sides have one; otherwise every field must
    /// match and there must be a `modifiedTime` to go on (an item Drive says
    /// nothing about is always fetched again).
    fn same_content(&self, then: &Self) -> bool {
        if let (Some(a), Some(b)) = (&self.sha256_checksum, &then.sha256_checksum) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (&self.md5_checksum, &then.md5_checksum) {
            return a == b;
        }
        self.modified_time.is_some() && self == then
    }
}

/// The local file as `+sync` last wrote (or verified) it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalFingerprint {
    size: u64,
    /// RFC 3339 modification time, nanosecond precision.
    modified: String,
    /// Lowercase hex SHA-256 of the content.
    sha256: String,
}

/// One mirrored Drive item.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SyncRecord {
    /// Local path relative to `--dir`, components joined by `/`. Only ever
    /// compared with paths built from the listing, never opened.
    path: String,
    remote: RemoteFingerprint,
    local: LocalFingerprint,
}

/// On-disk form of [`SYNC_STATE_FILE`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SyncStateFile {
    version: u32,
    folder_id: String,
    /// Keyed by Drive file ID.
    files: std::collections::BTreeMap<String, SyncRecord>,
}

/// The loaded sync state, with a path index.
struct SyncState {
    file: PathBuf,
    folder_id: String,
    records: std::collections::BTreeMap<String, SyncRecord>,
    by_path: std::collections::HashMap<String, String>,
    /// Records whose item moved to another path during this run, by their
    /// old path: a pristine copy left there may still be replaced.
    moved_from: std::collections::HashMap<String, (String, SyncRecord)>,
    dirty: bool,
}

impl SyncState {
    /// Load the state for `folder_id` from `dir`. A missing file is an empty
    /// state. A file that is corrupt, too large, of another schema version or
    /// for another folder is reported on stderr and replaced: every existing
    /// local file is then untracked, so none is overwritten without
    /// `--overwrite`. A file that exists but cannot be read is an error.
    fn load(dir: &Path, folder_id: &str) -> Result<Self, GwsError> {
        let mut state = Self {
            file: dir.join(SYNC_STATE_FILE),
            folder_id: folder_id.to_string(),
            records: std::collections::BTreeMap::new(),
            by_path: std::collections::HashMap::new(),
            moved_from: std::collections::HashMap::new(),
            dirty: false,
        };
        let Some(bytes) = Self::read(&state.file)? else {
            return Ok(state);
        };
        let parsed = bytes
            .and_then(|bytes| {
                serde_json::from_slice::<SyncStateFile>(&bytes)
                    .map_err(|e| format!("it is not valid sync state ({e})"))
            })
            .and_then(|f| {
                if f.version != SYNC_STATE_VERSION {
                    Err(format!(
                        "it has schema version {}, expected {SYNC_STATE_VERSION}",
                        f.version
                    ))
                } else if f.folder_id != folder_id {
                    Err(format!("it belongs to folder '{}'", f.folder_id))
                } else {
                    Ok(f.files)
                }
            });
        match parsed {
            Ok(records) => {
                for (id, rec) in records {
                    state.insert(id, rec);
                }
                state.moved_from.clear();
                state.dirty = false;
            }
            Err(why) => {
                tracing::warn!(
                    "ignoring sync state '{}': {why}; existing local files are treated as untracked and the state is rewritten",
                    state.file.display()
                );
                state.dirty = true;
            }
        }
        Ok(state)
    }

    /// The state file's bytes, `None` when it does not exist, or the reason
    /// it is not worth parsing.
    fn read(file: &Path) -> Result<Option<Result<Vec<u8>, String>>, GwsError> {
        let io_err = |e: std::io::Error| {
            GwsError::other(anyhow::anyhow!(
                "cannot read sync state '{}': {e}",
                file.display()
            ))
        };
        let meta = match std::fs::metadata(file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(e)),
        };
        if !meta.is_file() {
            return Err(GwsError::Validation(format!(
                "sync state '{}' is not a regular file; move it out of the way",
                file.display()
            )));
        }
        if meta.len() > SYNC_STATE_MAX_BYTES {
            return Ok(Some(Err(format!(
                "it is larger than {SYNC_STATE_MAX_BYTES} bytes"
            ))));
        }
        std::fs::read(file).map(|b| Some(Ok(b))).map_err(io_err)
    }

    /// The ID and record that last wrote `path`.
    fn at_path(&self, path: &str) -> Option<(&str, &SyncRecord)> {
        match self.by_path.get(path) {
            Some(id) => self.records.get(id).map(|r| (id.as_str(), r)),
            None => self.moved_from.get(path).map(|(id, r)| (id.as_str(), r)),
        }
    }

    /// Record `id` at `rec.path`, superseding whatever was recorded for that
    /// ID or that path before.
    fn insert(&mut self, id: String, rec: SyncRecord) {
        if self.records.get(&id) == Some(&rec) {
            return;
        }
        if let Some(old) = self.records.remove(&id) {
            self.by_path.remove(&old.path);
            if old.path != rec.path {
                self.moved_from.insert(old.path.clone(), (id.clone(), old));
            }
        }
        if let Some(other) = self.by_path.remove(&rec.path) {
            self.records.remove(&other);
        }
        self.moved_from.remove(&rec.path);
        self.by_path.insert(rec.path.clone(), id.clone());
        self.records.insert(id, rec);
        self.dirty = true;
    }

    /// Write the state atomically if anything changed.
    async fn save(&mut self) -> Result<(), GwsError> {
        if !self.dirty {
            return Ok(());
        }
        let body = SyncStateFile {
            version: SYNC_STATE_VERSION,
            folder_id: self.folder_id.clone(),
            files: self.records.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&body).map_err(|e| {
            GwsError::other(anyhow::anyhow!("cannot serialize the sync state: {e}"))
        })?;
        crate::output_file::write_atomic(&self.file, &bytes, true).await?;
        self.dirty = false;
        Ok(())
    }
}

/// Size and modification time of a local file, or `None` when it does not
/// exist. A file that exists but cannot be inspected is an error rather
/// than a reason to download over it.
fn local_stat(path: &Path) -> Result<Option<(u64, String)>, GwsError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(GwsError::other(anyhow::anyhow!(
                "cannot inspect '{}': {e}",
                path.display()
            )));
        }
    };
    let modified = meta.modified().map_err(|e| {
        GwsError::other(anyhow::anyhow!(
            "cannot read the modification time of '{}': {e}",
            path.display()
        ))
    })?;
    let modified = chrono::DateTime::<chrono::Utc>::from(modified)
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    Ok(Some((meta.len(), modified)))
}

/// Lowercase hex SHA-256 of a local file.
fn sha256_file(path: &Path) -> Result<String, GwsError> {
    use std::io::Read;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let err = |e: std::io::Error| {
        GwsError::other(anyhow::anyhow!("cannot read '{}': {e}", path.display()))
    };
    let mut file = std::fs::File::open(path).map_err(err)?;
    let mut ctx = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let mut out = String::with_capacity(64);
    for b in ctx.finish().as_ref() {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    Ok(out)
}

/// The fingerprint of the local file at `path`, read after a download.
fn local_fingerprint(path: &Path) -> Result<LocalFingerprint, GwsError> {
    let (size, modified) = local_stat(path)?.ok_or_else(|| {
        GwsError::other(anyhow::anyhow!(
            "'{}' disappeared right after it was written",
            path.display()
        ))
    })?;
    Ok(LocalFingerprint {
        size,
        modified,
        sha256: sha256_file(path)?,
    })
}

/// Whether the local file still holds what `+sync` recorded, and its
/// current fingerprint if so. Equal size and mtime are trusted; otherwise
/// the content is hashed, so a `touch` or a copy that keeps the bytes is
/// not mistaken for an edit.
fn unchanged_locally(
    path: &Path,
    stat: (u64, String),
    recorded: &LocalFingerprint,
) -> Result<Option<LocalFingerprint>, GwsError> {
    let (size, modified) = stat;
    if size != recorded.size {
        return Ok(None);
    }
    if modified == recorded.modified {
        return Ok(Some(recorded.clone()));
    }
    let sha256 = sha256_file(path)?;
    Ok((sha256 == recorded.sha256).then_some(LocalFingerprint {
        size,
        modified,
        sha256,
    }))
}

/// What `+sync` does with one Drive file.
#[derive(Debug, PartialEq, Eq)]
enum SyncAction {
    Download,
    /// Up to date; record this fingerprint (it may have a fresher mtime).
    Unchanged(LocalFingerprint),
    /// A local file that must not be overwritten is in the way.
    Conflict {
        reason: &'static str,
        remote_changed: Option<bool>,
    },
}

/// Decide what to do with Drive item `id` (fingerprint `remote`) whose
/// local copy belongs at `path` (`rel` relative to `--dir`).
fn plan_sync(
    state: &SyncState,
    id: &str,
    remote: &RemoteFingerprint,
    path: &Path,
    rel: &str,
) -> Result<SyncAction, GwsError> {
    let Some(stat) = local_stat(path)? else {
        return Ok(SyncAction::Download);
    };
    // A directory can never be replaced by a download.
    crate::validate::reject_directory(path, "sync target")?;
    match state.at_path(rel) {
        Some((owner, rec)) => {
            // Same item with the same content in Drive: nothing to fetch.
            // Otherwise Drive changed, or another item (a replacement with
            // the same name) now owns the path.
            let remote_same = owner == id && remote.same_content(&rec.remote);
            Ok(match unchanged_locally(path, stat, &rec.local)? {
                Some(local) if remote_same => SyncAction::Unchanged(local),
                Some(_) => SyncAction::Download,
                None => SyncAction::Conflict {
                    reason: "modifiedLocally",
                    remote_changed: Some(!remote_same),
                },
            })
        }
        None => {
            // The state keeps one path per ID, so a copy of the same item at
            // another path (a file with two parents in the synced tree, or
            // a rename only in case on a case-insensitive file system) has
            // no record here. It was still written by the sync when it holds
            // exactly what the item's record says the sync wrote.
            if let Some(rec) = state.records.get(id)
                && let Some(local) = unchanged_locally(path, stat.clone(), &rec.local)?
            {
                return Ok(if remote.same_content(&rec.remote) {
                    SyncAction::Unchanged(local)
                } else {
                    SyncAction::Download
                });
            }
            // Not written by a previous sync (or the state was lost). Adopt
            // it when Drive's checksum proves it already is the same file.
            if let Some(expected) = &remote.sha256_checksum {
                let sha256 = sha256_file(path)?;
                if sha256.eq_ignore_ascii_case(expected) {
                    let (size, modified) = stat;
                    return Ok(SyncAction::Unchanged(LocalFingerprint {
                        size,
                        modified,
                        sha256,
                    }));
                }
            }
            Ok(SyncAction::Conflict {
                reason: "untracked",
                remote_changed: None,
            })
        }
    }
}

/// What `+sync` does with one Drive child.
#[derive(Clone, Copy)]
enum SyncKind {
    Folder,
    Export(&'static ExportFormat),
    Media,
}

/// Final local names for the children of one folder, in input order.
///
/// Files and sub-folders share one namespace on disk, so a name claimed by
/// more than one child (after sanitizing and adding export extensions) is
/// given to none of them: every claimant becomes `{id}-{name}`. This keeps
/// the layout independent of the order Drive lists children in, and a plain
/// name always belongs to the only item that has it, so a later run never
/// mistakes one item's local copy for another's. A prefixed name that still
/// collides (an item literally named `{other id}-{name}`) is an error rather
/// than a silent overwrite.
///
/// Names are compared case-insensitively: Drive names are case-sensitive,
/// but the default macOS and Windows file systems are not, so `Report.txt`
/// and `report.txt` would otherwise share one local file (see
/// [`case_insensitive_key`]).
fn disambiguate(children: &[(&str, &str)]) -> Result<Vec<String>, GwsError> {
    let mut claims: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for &(_, name) in children {
        *claims.entry(case_insensitive_key(name)).or_insert(0) += 1;
    }
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(children.len());
    for &(id, name) in children {
        let local = if matches!(claims.get(&case_insensitive_key(name)), Some(&n) if n > 1) {
            safe_filename(&format!("{id}-{name}"), id)
        } else {
            name.to_string()
        };
        if !used.insert(case_insensitive_key(&local)) {
            return Err(GwsError::other(anyhow::anyhow!(
                "cannot sync: more than one Drive item in the same folder maps to the local name '{local}' (item '{id}'); rename one in Drive"
            )));
        }
        out.push(local);
    }
    Ok(out)
}

/// The error `+sync` exits with after printing its report, when any file
/// failed or was kept as a conflict. Like `gwsr batch`'s
/// `batchPartialFailure`: the report on stdout says what was mirrored and
/// what was not, and the exit code is still `1`.
fn sync_failure(report: &Value) -> Option<GwsError> {
    let count = |key: &str| {
        report
            .get(key)
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    let failed = count("failed");
    let conflicts = count("conflicts");
    let conflict_note = format!(
        "{conflicts} local file(s) changed since the last sync or not written by it were kept; see \"conflicts\" in the report on stdout, and pass --overwrite to replace them with the Drive copy"
    );
    if failed > 0 {
        let mut message =
            format!("{failed} file(s) could not be synced; see \"failed\" in the report on stdout");
        if conflicts > 0 {
            message = format!("{message}. {conflict_note}");
        }
        Some(GwsError::Api {
            code: 207,
            message,
            reason: "syncPartialFailure".to_string(),
            enable_url: None,
        })
    } else if conflicts > 0 {
        Some(GwsError::Api {
            code: 409,
            message: conflict_note,
            reason: "syncConflict".to_string(),
            enable_url: None,
        })
    } else {
        None
    }
}

/// `+sync` with the default of keeping conflicting local files.
#[cfg(test)]
async fn sync(api: &Api, folder_id: &str, dir: &Path) -> Result<Value, GwsError> {
    sync_uninterrupted(api, folder_id, dir, false).await
}

/// `+sync` that no signal interrupts, so tests leave the process's signal
/// handling alone.
#[cfg(test)]
#[allow(clippy::panic)]
async fn sync_uninterrupted(
    api: &Api,
    folder_id: &str,
    dir: &Path,
    overwrite: bool,
) -> Result<Value, GwsError> {
    match run_sync_until(api, folder_id, dir, overwrite, std::future::pending()).await? {
        SyncEnd::Done(report) => Ok(report),
        SyncEnd::Interrupted(signal) => panic!("a pending future never interrupts ({signal:?})"),
    }
}

/// How a `+sync` run ended.
#[derive(Debug)]
enum SyncEnd {
    /// The walk finished; the report to print.
    Done(Value),
    /// A signal stopped the walk; the state of what finished was saved.
    Interrupted(crate::helpers::ShutdownSignal),
}

/// [`run_sync_until`] a SIGINT or SIGTERM.
async fn run_sync(
    api: &Api,
    folder_id: &str,
    dir: &Path,
    overwrite: bool,
) -> Result<SyncEnd, GwsError> {
    run_sync_until(
        api,
        folder_id,
        dir,
        overwrite,
        crate::helpers::shutdown_signal(),
    )
    .await
}

/// Fail unless `folder_id` is a folder the user can read. Listing its
/// children cannot tell: `'ID' in parents` matches nothing, without an error,
/// for an ID that does not exist or is no longer shared, and for a file.
async fn check_sync_folder(api: &Api, folder_id: &str) -> Result<(), GwsError> {
    let meta = api
        .send(
            ApiRequest::get(api.url(&format!("files/{}", encode_path_segment(folder_id))))
                .query("fields", "id,mimeType")
                .query("supportsAllDrives", "true"),
        )
        .await?;
    if api.is_dry_run() {
        return Ok(());
    }
    let mime = str_field(&meta, "mimeType")?;
    if mime != FOLDER_MIME {
        return Err(GwsError::Validation(format!(
            "--folder-id '{folder_id}' is not a folder (its type is {mime}); +sync mirrors a Drive folder"
        )));
    }
    Ok(())
}

/// Mirror `folder_id` into `dir` until `interrupt` completes, then save the
/// sync state, also when the walk was aborted part-way or interrupted, so
/// files already mirrored stay tracked.
///
/// Stopping the walk at any await is safe: a download is written to a
/// temporary file renamed into place only once complete, and a finished
/// download is recorded in `state` without an await in between.
async fn run_sync_until(
    api: &Api,
    folder_id: &str,
    dir: &Path,
    overwrite: bool,
    interrupt: impl Future<Output = crate::helpers::ShutdownSignal>,
) -> Result<SyncEnd, GwsError> {
    check_sync_folder(api, folder_id).await?;
    if api.is_dry_run() {
        list_children(api, folder_id).await?;
        api.plan_note(json!({
            "note": "each child file is downloaded (or exported) into the directory; sub-folders are recursed",
            "dir": dir.display().to_string(),
            "state": dir.join(SYNC_STATE_FILE).display().to_string(),
        }))?;
        return Ok(SyncEnd::Done(Value::Null));
    }
    let _lock = lock_sync_dir(dir)?;
    let mut state = SyncState::load(dir, folder_id)?;
    let walked = tokio::select! {
        biased;
        walked = sync_walk(api, folder_id, dir, overwrite, &mut state) => walked.map(SyncEnd::Done),
        signal = interrupt => Ok(SyncEnd::Interrupted(signal)),
    };
    let saved = state.save().await;
    match (walked, saved) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(err), Ok(())) | (Ok(_), Err(err)) => Err(err),
        (Err(err), Err(save_err)) => {
            // The walk's error is the one to exit with; say loudly that
            // the state of this partial run could not be kept either.
            tracing::error!("{save_err}");
            Err(err)
        }
    }
}

/// Take the `+sync` lock of `dir` (created if missing), or fail at once when
/// another run holds it. It is released when the returned lock is dropped,
/// also when the process dies.
fn lock_sync_dir(dir: &Path) -> Result<crate::fs_util::FileLock, GwsError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        GwsError::other(anyhow::anyhow!(
            "cannot create the sync directory '{}': {e}",
            dir.display()
        ))
    })?;
    let file = dir.join(SYNC_LOCK_FILE);
    crate::fs_util::FileLock::acquire(&file, std::time::Duration::ZERO).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            GwsError::Validation(format!(
                "another drive +sync is running in '{}' (it holds '{}'); wait for it to finish",
                dir.display(),
                file.display()
            ))
        } else {
            GwsError::other(anyhow::anyhow!("cannot lock '{}': {e}", file.display()))
        }
    })
}

async fn sync_walk(
    api: &Api,
    folder_id: &str,
    dir: &Path,
    overwrite: bool,
    state: &mut SyncState,
) -> Result<Value, GwsError> {
    let mut downloaded = Vec::new();
    let mut unchanged = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut conflicts = Vec::new();
    // (folder ID, local directory, path relative to `dir` joined by '/')
    let mut stack: Vec<(String, PathBuf, String)> =
        vec![(folder_id.to_string(), dir.to_path_buf(), String::new())];
    // Folder ID -> the local directory it is mirrored into.
    let mut visited: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();

    while let Some((id, local_dir, rel_dir)) = stack.pop() {
        // A folder with more than one parent in the tree is mirrored once;
        // say where, rather than silently leaving the other path empty.
        if let Some(first) = visited.get(&id) {
            skipped.push(json!({
                "id": id,
                "path": local_dir.display().to_string(),
                "mimeType": FOLDER_MIME,
                "reason": format!("folder already mirrored at '{}'", first.display()),
            }));
            continue;
        }
        visited.insert(id.clone(), local_dir.clone());
        let mut entries = Vec::new();
        for child in list_children(api, &id).await? {
            let child_id = str_field(&child, "id")?;
            let name = str_field(&child, "name")?;
            let mime = str_field(&child, "mimeType")?;
            // NFC so that names equal on macOS file systems compare equal below.
            let local_name = nfc(&safe_filename(name, child_id)).into_owned();
            let kind = if mime == FOLDER_MIME {
                SyncKind::Folder
            } else if mime.starts_with("application/vnd.google-apps.") {
                match sync_export(mime) {
                    Some(f) => SyncKind::Export(f),
                    None => {
                        skipped.push(json!({"id": child_id, "name": name, "mimeType": mime, "reason": "Google-native type with no export"}));
                        continue;
                    }
                }
            } else {
                SyncKind::Media
            };
            let local_name = match kind {
                SyncKind::Export(f) => with_extension(&local_name, f.ext),
                SyncKind::Folder | SyncKind::Media => local_name,
            };
            entries.push((child_id.to_string(), local_name, kind, child));
        }
        let keys: Vec<(&str, &str)> = entries
            .iter()
            .map(|(cid, n, _, _)| (cid.as_str(), n.as_str()))
            .collect();
        let names = disambiguate(&keys)?;
        for ((child_id, _, kind, child), local_name) in entries.iter().zip(names) {
            let child_id = child_id.as_str();
            let rel = if rel_dir.is_empty() {
                local_name.clone()
            } else {
                format!("{rel_dir}/{local_name}")
            };
            let export = match kind {
                SyncKind::Folder => {
                    stack.push((child_id.to_string(), local_dir.join(&local_name), rel));
                    continue;
                }
                SyncKind::Export(f) => Some(*f),
                SyncKind::Media => None,
            };
            let path = local_dir.join(&local_name);
            let remote = RemoteFingerprint::of(child);
            match plan_sync(state, child_id, &remote, &path, &rel)? {
                SyncAction::Download => {}
                SyncAction::Unchanged(local) => {
                    state.insert(
                        child_id.to_string(),
                        SyncRecord {
                            path: rel,
                            remote,
                            local,
                        },
                    );
                    unchanged.push(path.display().to_string());
                    continue;
                }
                SyncAction::Conflict { .. } if overwrite => {}
                SyncAction::Conflict {
                    reason,
                    remote_changed,
                } => {
                    let mut entry = json!({"id": child_id, "path": path.display().to_string(), "reason": reason});
                    if let Some(changed) = remote_changed {
                        entry["remoteChanged"] = json!(changed);
                    }
                    conflicts.push(entry);
                    continue;
                }
            }
            let target = OutputTarget::File {
                path: path.clone(),
                overwrite: true,
            };
            let req = match export {
                Some(f) => ApiRequest::get(
                    api.url(&format!("files/{}/export", encode_path_segment(child_id))),
                )
                .query("mimeType", f.mime),
                None => {
                    ApiRequest::get(api.url(&format!("files/{}", encode_path_segment(child_id))))
                        .query("alt", "media")
                        .query("supportsAllDrives", "true")
                }
            };
            match api.download(req, &target).await {
                Ok(bytes) => {
                    let local = local_fingerprint(&path)?;
                    state.insert(
                        child_id.to_string(),
                        SyncRecord {
                            path: rel,
                            remote,
                            local,
                        },
                    );
                    downloaded.push(
                        json!({"id": child_id, "path": path.display().to_string(), "bytes": bytes}),
                    );
                }
                // The API refused this one file (restricted download, export
                // size limit...): record it and mirror the rest. Anything
                // else (auth, network, local I/O, and retryable API errors
                // such as rate limits or 503s) affects every file, so it
                // aborts the sync with its own exit code.
                Err(err @ GwsError::Api { .. }) if !err.is_retryable() => failed.push(json!({
                    "id": child_id,
                    "path": path.display().to_string(),
                    "error": err.to_json()["error"],
                })),
                Err(err) => return Err(err),
            }
        }
    }
    Ok(json!({
        "downloaded": downloaded,
        "unchanged": unchanged,
        "skipped": skipped,
        "conflicts": conflicts,
        "failed": failed,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn matches(args: &[&str]) -> ArgMatches {
        let cmd = DriveHelper.inject_commands(
            Command::new("gwsr").arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue)
                    .global(true),
            ),
            &crate::discovery::RestDescription::default(),
        );
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn determine_filename_from_path() {
        assert_eq!(determine_filename("path/to/file.txt").unwrap(), "file.txt");
        assert!(determine_filename("").is_err());
        assert!(determine_filename("/").is_err());
    }

    #[test]
    fn metadata_with_parent_and_conversion() {
        let args = UploadArgs {
            path: PathBuf::from("a.csv"),
            name: "a".into(),
            folder_id: Some("F".into()),
            content_type: "text/csv".into(),
            convert: true,
        };
        let meta = build_metadata(&args).unwrap();
        assert_eq!(meta["parents"][0], "F");
        assert_eq!(meta["mimeType"], SHEET);
    }

    #[test]
    fn metadata_conversion_unsupported_type_fails() {
        let args = UploadArgs {
            path: PathBuf::from("a.bin"),
            name: "a".into(),
            folder_id: None,
            content_type: "application/octet-stream".into(),
            convert: true,
        };
        assert!(build_metadata(&args).is_err());
    }

    /// `--convert` compares media types case-insensitively and without
    /// parameters, and knows the macro-enabled, template and slideshow Office
    /// types Drive imports (as detected from their extensions).
    #[test]
    fn conversion_target_ignores_case_and_parameters_and_knows_office_variants() {
        for (content_type, target) in [
            ("text/csv; charset=utf-8", SHEET),
            ("Text/CSV", SHEET),
            ("TEXT/PLAIN;charset=UTF-8", DOC),
        ] {
            assert_eq!(
                conversion_target(content_type),
                Some(target),
                "{content_type}"
            );
        }
        for (file, target) in [
            ("a.xlsm", SHEET),
            ("a.xltx", SHEET),
            ("a.xltm", SHEET),
            ("a.docm", DOC),
            ("a.dotx", DOC),
            ("a.dotm", DOC),
            ("a.pptm", SLIDES),
            ("a.potx", SLIDES),
            ("a.potm", SLIDES),
            ("a.ppsx", SLIDES),
            ("a.ppsm", SLIDES),
        ] {
            let detected = mime_guess2::from_path(file).first().unwrap().to_string();
            assert_eq!(
                conversion_target(&detected),
                Some(target),
                "{file}: {detected}"
            );
        }
        assert_eq!(conversion_target("application/pdf"), None);
        assert_eq!(conversion_target("text/csvx"), None);
    }

    #[tokio::test]
    async fn upload_uses_shared_drive_resumable_session() {
        let server = MockServer::start().await;
        let session = format!("{}/upload/drive/v3/files?upload_id=xyz", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/drive/v3/files"))
            .and(query_param("uploadType", "resumable"))
            .and(query_param("supportsAllDrives", "true"))
            .and(body_json(
                json!({"name": "r.txt", "parents": ["SD_FOLDER"]}),
            ))
            .respond_with(ResponseTemplate::new(200).insert_header("location", session.as_str()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload/drive/v3/files"))
            .and(query_param("upload_id", "xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "NEW"})))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("r.txt");
        std::fs::write(&file, "data").unwrap();
        let api = api(&server.uri(), "drive/v3/");
        let args = UploadArgs {
            path: file,
            name: "r.txt".into(),
            folder_id: Some("SD_FOLDER".into()),
            content_type: "text/plain".into(),
            convert: false,
        };
        let v = upload(&api, &args).await.unwrap();
        assert_eq!(v["id"], "NEW");
    }

    /// `--mime-type` goes into the `X-Upload-Content-Type` header: control
    /// characters and anything that is not a `type/subtype` media type are
    /// refused up front, before credentials are loaded.
    #[test]
    fn upload_rejects_an_invalid_mime_type() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("r.txt");
        std::fs::write(&file, "data").unwrap();
        let file = file.to_str().unwrap();
        for bad in [
            "text/plain\r\nX-Evil: 1",
            "text/plain\n",
            "plain",
            "",
            "text/",
        ] {
            let m = matches(&["gwsr", "+upload", "--file", file, "--mime-type", bad]);
            let (_, sub) = m.subcommand().unwrap();
            let err = UploadArgs::parse(sub)
                .err()
                .unwrap_or_else(|| panic!("{bad:?} accepted"));
            assert!(matches!(err, GwsError::Validation(_)), "{bad:?}: {err:?}");
            assert!(err.to_string().contains("--mime-type"), "{err}");
        }
        for good in [
            "text/plain",
            "text/csv; charset=utf-8",
            "application/vnd.ms-excel.sheet.macroEnabled.12",
        ] {
            let m = matches(&["gwsr", "+upload", "--file", file, "--mime-type", good]);
            let (_, sub) = m.subcommand().unwrap();
            assert_eq!(UploadArgs::parse(sub).unwrap().content_type, good);
        }
    }

    #[tokio::test]
    async fn download_rejects_native_files() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/DOC1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": "DOC1", "name": "Doc", "mimeType": DOC})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let err = download(&api, "DOC1", Some("-"), false).await.unwrap_err();
        assert!(err.to_string().contains("+export"));
    }

    /// Google-native types `+export` cannot handle must not be sent there:
    /// a shortcut points at its target, anything else is plainly refused.
    #[tokio::test]
    async fn download_of_non_exportable_native_files_does_not_suggest_export() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/SC1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "SC1", "name": "Link", "mimeType": "application/vnd.google-apps.shortcut",
                "shortcutDetails": {"targetId": "TARGET1", "targetMimeType": "application/pdf"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/FORM1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "FORM1", "name": "Survey", "mimeType": "application/vnd.google-apps.form"
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");

        let err = download(&api, "SC1", Some("-"), false).await.unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(!msg.contains("+export"), "{msg}");
        assert!(msg.contains("--file-id TARGET1"), "{msg}");

        let err = download(&api, "FORM1", Some("-"), false).await.unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(!msg.contains("+export"), "{msg}");
        assert!(msg.contains("cannot be downloaded or exported"), "{msg}");
    }

    /// `+export` of a shortcut points at the shortcut's target, and a
    /// Google-native type that cannot be exported is not sent to `+download`,
    /// which refuses it too.
    #[tokio::test]
    async fn export_of_a_shortcut_or_non_exportable_native_file_names_a_command_that_works() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/SC1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "SC1", "name": "Link", "mimeType": "application/vnd.google-apps.shortcut",
                "shortcutDetails": {"targetId": "TARGET1", "targetMimeType": DOC}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/FORM1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "FORM1", "name": "Survey", "mimeType": "application/vnd.google-apps.form"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/PDF1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "PDF1", "name": "Paper", "mimeType": "application/pdf"
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");

        let err = export(&api, "SC1", "pdf", Some("-"), false)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(msg.contains("+export --file-id TARGET1 --to pdf"), "{msg}");
        assert!(!msg.contains("+download"), "{msg}");

        let err = export(&api, "FORM1", "pdf", Some("-"), false)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(!msg.contains("+download"), "{msg}");
        assert!(msg.contains("cannot be downloaded or exported"), "{msg}");

        // A regular file is still sent to +download.
        let err = export(&api, "PDF1", "pdf", Some("-"), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("+download"), "{err}");
    }

    #[tokio::test]
    async fn download_streams_alt_media() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/F1"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PDFDATA".to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/F1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"id": "F1", "name": "a.pdf", "mimeType": "application/pdf"}),
                ),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.pdf");
        // OutputTarget::parse restricts to CWD, so drive the lower-level API.
        let target = OutputTarget::File {
            path: out.clone(),
            overwrite: false,
        };
        let bytes = api
            .download(
                ApiRequest::get(api.url("files/F1"))
                    .query("alt", "media")
                    .query("supportsAllDrives", "true"),
                &target,
            )
            .await
            .unwrap();
        assert_eq!(bytes, Some(7));
        assert_eq!(std::fs::read(out).unwrap(), b"PDFDATA");
    }

    #[test]
    fn export_format_validation() {
        let md = export_format("md").unwrap();
        assert!(check_export(md, DOC).is_ok());
        let err = check_export(md, SHEET).unwrap_err().to_string();
        assert!(err.contains("csv"), "{err}");
        assert!(check_export(md, "application/pdf").is_err());
        assert_eq!(with_extension("Report", "pdf"), "Report.pdf");
        assert_eq!(with_extension("Report.PDF", "pdf"), "Report.PDF");
    }

    #[test]
    fn export_default_name_keeps_the_extension_for_long_titles() {
        // Shortening a long Drive title must not cut off the export format's
        // extension (`report.pdf` saved as `report` with no extension).
        let long = "Quarterly report ".repeat(20);
        let target = resolve_target(None, &long, Some("pdf"), false).unwrap();
        let OutputTarget::File { path, .. } = target else {
            panic!("expected a file target");
        };
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(".pdf"), "{name}");
        assert!(
            name.len() <= http::SAFE_FILENAME_MAX_BYTES + ".pdf".len(),
            "{name}"
        );
    }

    #[tokio::test]
    async fn export_dry_run_plans_export_request() {
        let api = dry_api("drive/v3/");
        export(&api, "DOC1", "pdf", Some("-"), false).await.unwrap();
        let plan = api.planned();
        assert_eq!(plan.len(), 1);
        assert!(
            plan[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/files/DOC1/export")
        );
        assert_eq!(plan[0]["query_params"]["mimeType"], "application/pdf");
    }

    #[tokio::test]
    async fn move_replaces_parents() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/F1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"id": "F1", "name": "a", "mimeType": "text/plain", "parents": ["P1", "P2"]}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/drive/v3/files/F1"))
            .and(query_param("addParents", "DEST"))
            .and(query_param("removeParents", "P1,P2"))
            .and(query_param("supportsAllDrives", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "F1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        move_file(&api, "F1", "DEST").await.unwrap();
    }

    #[test]
    fn share_requires_role_and_grantee() {
        let cmd = DriveHelper.inject_commands(
            Command::new("gwsr"),
            &crate::discovery::RestDescription::default(),
        );
        assert!(
            cmd.clone()
                .try_get_matches_from(["gwsr", "+share", "--file-id", "F", "--email", "a@b.c"])
                .is_err()
        );
        assert!(
            cmd.try_get_matches_from(["gwsr", "+share", "--file-id", "F", "--role", "reader"])
                .is_err()
        );
    }

    fn share_args(args: &[&str]) -> ShareArgs {
        let m = matches(args);
        ShareArgs::parse(m.subcommand_matches("+share").unwrap()).unwrap()
    }

    #[tokio::test]
    async fn share_owner_is_destructive_and_transfers() {
        let args = share_args(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--email",
            "a@b.c",
            "--role",
            "owner",
        ]);
        assert_eq!(args.impact().0, Impact::Destructive);
        let api = dry_api("drive/v3/");
        share(&api, &args).await.unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["query_params"]["transferOwnership"], "true");
        assert_eq!(plan[0]["body"]["type"], "user");
    }

    #[test]
    fn share_impact_classification() {
        let broad_writer = share_args(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--anyone",
            "--role",
            "writer",
        ]);
        assert_eq!(broad_writer.impact().0, Impact::Destructive);
        let broad_reader = share_args(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--anyone",
            "--role",
            "reader",
        ]);
        assert_eq!(broad_reader.impact().0, Impact::Outbound);
        let user_writer = share_args(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--email",
            "a@b.c",
            "--role",
            "writer",
        ]);
        assert_eq!(user_writer.impact().0, Impact::Outbound);
        assert!(user_writer.impact().1.contains("a@b.c"));
    }

    #[test]
    fn share_gate_refuses_destructive_without_yes() {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            return;
        }
        let m = matches(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--anyone",
            "--role",
            "writer",
        ]);
        let sub = m.subcommand_matches("+share").unwrap();
        let (impact, action) = ShareArgs::parse(sub).unwrap().impact();
        assert!(confirm::confirm(sub, impact, &action).is_err());
        let m = matches(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--anyone",
            "--role",
            "writer",
            "--yes",
        ]);
        let sub = m.subcommand_matches("+share").unwrap();
        assert!(confirm::confirm(sub, impact, &action).is_ok());
    }

    #[tokio::test]
    async fn share_anyone_reader_body() {
        let api = dry_api("drive/v3/");
        let args = share_args(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--anyone",
            "--role",
            "reader",
        ]);
        share(&api, &args).await.unwrap();
        assert_eq!(
            api.planned()[0]["body"],
            json!({"role": "reader", "type": "anyone"})
        );
    }

    #[tokio::test]
    async fn share_group_sends_notification_params() {
        let api = dry_api("drive/v3/");
        let m = matches(&[
            "gwsr",
            "+share",
            "--file-id",
            "F",
            "--email",
            "g@b.c",
            "--group",
            "--role",
            "writer",
            "--no-notify",
        ]);
        let args = ShareArgs::parse(m.subcommand_matches("+share").unwrap()).unwrap();
        share(&api, &args).await.unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["body"]["type"], "group");
        assert_eq!(plan[0]["query_params"]["sendNotificationEmail"], "false");
    }

    #[tokio::test]
    async fn sync_downloads_and_exports_recursively() {
        let server = MockServer::start().await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "A", "name": "a.txt", "mimeType": "text/plain", "modifiedTime": "2020-01-01T00:00:00Z"},
                {"id": "D", "name": "Notes", "mimeType": DOC, "modifiedTime": "2020-01-01T00:00:00Z"},
                {"id": "S", "name": "sub", "mimeType": FOLDER_MIME},
                {"id": "X", "name": "Form", "mimeType": "application/vnd.google-apps.form"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'S' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "B", "name": "b.bin", "mimeType": "application/octet-stream"}
            ]})))
            .mount(&server)
            .await;
        for id in ["A", "B"] {
            Mock::given(method("GET"))
                .and(path(format!("/drive/v3/files/{id}")))
                .and(query_param("alt", "media"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(id.as_bytes().to_vec()))
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/D/export"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"DOCX".to_vec()))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["downloaded"].as_array().unwrap().len(), 3);
        assert_eq!(v["skipped"].as_array().unwrap().len(), 1);
        assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"A");
        assert_eq!(
            std::fs::read(dir.path().join("Notes.docx")).unwrap(),
            b"DOCX"
        );
        assert_eq!(std::fs::read(dir.path().join("sub/b.bin")).unwrap(), b"B");

        // Second run: A and the Doc are recorded with an unchanged
        // modifiedTime; B has no fingerprint, so it is fetched again.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn sync_reports_a_failed_file_and_still_mirrors_the_rest() {
        let server = MockServer::start().await;
        // A restricted file the owner blocked from download, listed first.
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/R"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
                "code": 403,
                "message": "The user does not have sufficient permissions to download this file.",
                "errors": [{"reason": "cannotDownloadFile"}]
            }})))
            .mount(&server)
            .await;
        // A Doc too large to export.
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/BIG/export"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
                "code": 403,
                "message": "This file is too large to be exported.",
                "errors": [{"reason": "exportSizeLimitExceeded"}]
            }})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/OK"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"OK".to_vec()))
            .mount(&server)
            .await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "R", "name": "restricted.pdf", "mimeType": "application/pdf"},
                {"id": "BIG", "name": "Huge", "mimeType": DOC},
                {"id": "OK", "name": "ok.txt", "mimeType": "text/plain"}
            ]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();

        // The good file is mirrored and reported despite the failures.
        assert_eq!(std::fs::read(dir.path().join("ok.txt")).unwrap(), b"OK");
        let downloaded = v["downloaded"].as_array().unwrap();
        assert_eq!(downloaded.len(), 1);
        assert_eq!(downloaded[0]["id"], "OK");
        // Each failure names the item and the API's reason.
        let failed = v["failed"].as_array().unwrap();
        let mut reasons: Vec<(&str, &str)> = failed
            .iter()
            .map(|f| {
                (
                    f["id"].as_str().unwrap(),
                    f["error"]["reason"].as_str().unwrap(),
                )
            })
            .collect();
        reasons.sort_unstable();
        assert_eq!(
            reasons,
            [
                ("BIG", "exportSizeLimitExceeded"),
                ("R", "cannotDownloadFile")
            ]
        );
        assert!(!dir.path().join("restricted.pdf").exists());
        assert!(!dir.path().join("Huge.docx").exists());

        // The command still fails loudly, like `gwsr batch`, after the report.
        let err = sync_failure(&v).unwrap();
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
        assert_eq!(err.to_json()["error"]["reason"], "syncPartialFailure");
        assert!(err.to_string().contains("2 file(s)"), "{err}");
    }

    #[tokio::test]
    async fn sync_aborts_on_errors_that_are_not_per_file() {
        let server = MockServer::start().await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "A", "name": "a.txt", "mimeType": "text/plain"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/A"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"A".to_vec()))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        // The local target can't be written: a directory sits at its path.
        std::fs::create_dir(dir.path().join("a.txt")).unwrap();
        assert!(sync(&api, "ROOT", dir.path()).await.is_err());

        // A clean run is not a partial failure.
        assert!(sync_failure(&json!({"failed": []})).is_none());
    }

    /// A rate limit or server outage is not about one file: it would hit
    /// every later file too, and it is retryable (exit 6). It must abort the
    /// sync with that error, not be listed under "failed" and turned into a
    /// non-retryable `syncPartialFailure` (exit 1).
    #[tokio::test]
    async fn sync_aborts_on_retryable_api_errors() {
        let bodies = [
            (
                403,
                json!({"error": {"code": 403, "message": "User rate limit exceeded.",
                    "errors": [{"domain": "usageLimits", "reason": "userRateLimitExceeded"}]}}),
            ),
            (
                503,
                json!({"error": {"code": 503, "message": "Backend Error",
                    "errors": [{"reason": "backendError"}]}}),
            ),
        ];
        for (status, body) in bodies {
            let server = MockServer::start().await;
            mount_root(&server).await;
            Mock::given(method("GET"))
                .and(path("/drive/v3/files"))
                .and(query_param("q", "'ROOT' in parents and trashed = false"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                    {"id": "A", "name": "a.txt", "mimeType": "text/plain"}
                ]})))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/drive/v3/files/A"))
                .and(query_param("alt", "media"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&server)
                .await;
            let api = api(&server.uri(), "drive/v3/");
            let dir = tempfile::tempdir().unwrap();
            let err = sync(&api, "ROOT", dir.path())
                .await
                .expect_err(&format!("HTTP {status} must abort the sync"));
            assert_eq!(
                err.exit_code(),
                GwsError::EXIT_CODE_API_RETRYABLE,
                "HTTP {status}: {err:?}"
            );
        }
    }

    /// `files.list` with `'ID' in parents` answers an empty list, not an
    /// error, for a folder ID that does not exist or is no longer shared with
    /// the user, and for a file ID. That must not pass for "nothing to sync".
    #[tokio::test]
    async fn sync_fails_loudly_for_a_missing_folder_or_a_file() {
        let empty = json!({"files": []});
        // Gone or no longer accessible.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&empty))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/GONE"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": {
                "code": 404, "message": "File not found: GONE.",
                "errors": [{"reason": "notFound"}]}})))
            .mount(&server)
            .await;
        let gone = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let err = sync(&gone, "GONE", dir.path()).await.unwrap_err();
        assert_eq!(err.to_json()["error"]["code"], 404, "{err:?}");
        assert!(!dir.path().join(SYNC_STATE_FILE).exists());

        // A file, not a folder.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&empty))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/F"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": "F", "mimeType": "application/pdf"})),
            )
            .mount(&server)
            .await;
        let file = api(&server.uri(), "drive/v3/");
        let err = sync(&file, "F", dir.path()).await.unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        assert!(err.to_string().contains("not a folder"), "{err}");
    }

    /// The dry-run plan shows the folder lookup before the listing.
    #[tokio::test]
    async fn sync_dry_run_plans_the_folder_lookup() {
        let api = dry_api("drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        sync(&api, "ROOT", dir.path()).await.unwrap();
        let plan = api.planned();
        assert!(
            plan[0]["url"].as_str().unwrap().ends_with("/files/ROOT"),
            "{plan:?}"
        );
        assert_eq!(plan[0]["query_params"]["fields"], "id,mimeType");
        assert!(
            plan[1]["url"].as_str().unwrap().ends_with("/files"),
            "{plan:?}"
        );
    }

    /// Mounts the metadata of the synced folder `ROOT`, which `+sync` reads
    /// before it lists anything.
    async fn mount_root(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/ROOT"))
            .and(query_param("fields", "id,mimeType"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": "ROOT", "mimeType": FOLDER_MIME})),
            )
            .mount(server)
            .await;
    }

    /// Mounts a `files.list` response for `parent` and an `alt=media`
    /// download returning `id` as the body for each plain file listed.
    async fn mount_listing(server: &MockServer, parent: &str, files: Value) {
        if parent == "ROOT" {
            mount_root(server).await;
        }
        for f in files.as_array().unwrap() {
            if f["mimeType"] != FOLDER_MIME {
                let id = f["id"].as_str().unwrap().to_string();
                Mock::given(method("GET"))
                    .and(path(format!("/drive/v3/files/{id}")))
                    .and(query_param("alt", "media"))
                    .respond_with(ResponseTemplate::new(200).set_body_bytes(id.into_bytes()))
                    .mount(server)
                    .await;
            }
        }
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param(
                "q",
                format!("'{parent}' in parents and trashed = false"),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "files": files })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn sync_keeps_same_named_sibling_folders_apart() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "R1", "name": "Reports", "mimeType": FOLDER_MIME},
                {"id": "R2", "name": "Reports", "mimeType": FOLDER_MIME}
            ]),
        )
        .await;
        for (parent, file) in [("R1", "F1"), ("R2", "F2")] {
            mount_listing(
                &server,
                parent,
                json!([{"id": file, "name": "a.txt", "mimeType": "text/plain",
                        "modifiedTime": "2020-01-01T00:00:00Z"}]),
            )
            .await;
        }
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["downloaded"].as_array().unwrap().len(), 2, "{v}");
        assert!(v["unchanged"].as_array().unwrap().is_empty(), "{v}");
        // Colliding names are all prefixed, so neither folder owns the plain
        // name and the layout does not depend on listing order.
        assert!(!dir.path().join("Reports").exists());
        assert_eq!(
            std::fs::read(dir.path().join("R1-Reports/a.txt")).unwrap(),
            b"F1"
        );
        assert_eq!(
            std::fs::read(dir.path().join("R2-Reports/a.txt")).unwrap(),
            b"F2"
        );
    }

    /// A folder with two parents in the synced tree is mirrored once; the
    /// other path is reported under "skipped", not silently left empty.
    #[tokio::test]
    async fn sync_reports_a_folder_reached_through_a_second_parent() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "A", "name": "a", "mimeType": FOLDER_MIME},
                {"id": "B", "name": "b", "mimeType": FOLDER_MIME}
            ]),
        )
        .await;
        let shared = json!([{"id": "F", "name": "shared", "mimeType": FOLDER_MIME}]);
        mount_listing(&server, "A", shared.clone()).await;
        mount_listing(&server, "B", shared).await;
        mount_listing(
            &server,
            "F",
            json!([{"id": "X", "name": "x.txt", "mimeType": "text/plain"}]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["X"], "{v}");
        assert_eq!(ids(&v, "skipped"), ["F"], "{v}");
        let skipped = &v["skipped"][0];
        let mirrored = PathBuf::from(v["downloaded"][0]["path"].as_str().unwrap());
        let mirrored = mirrored.parent().unwrap();
        let other = if mirrored.starts_with(dir.path().join("a")) {
            dir.path().join("b").join("shared")
        } else {
            dir.path().join("a").join("shared")
        };
        assert_eq!(skipped["path"], other.display().to_string(), "{v}");
        assert!(
            skipped["reason"]
                .as_str()
                .unwrap()
                .contains(&mirrored.display().to_string()),
            "{v}"
        );
    }

    #[tokio::test]
    async fn sync_keeps_a_folder_and_file_with_one_name_apart() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "FILE", "name": "Reports", "mimeType": "text/plain"},
                {"id": "DIR", "name": "Reports", "mimeType": FOLDER_MIME}
            ]),
        )
        .await;
        mount_listing(
            &server,
            "DIR",
            json!([{"id": "IN", "name": "a.txt", "mimeType": "text/plain"}]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["downloaded"].as_array().unwrap().len(), 2, "{v}");
        assert_eq!(
            std::fs::read(dir.path().join("FILE-Reports")).unwrap(),
            b"FILE"
        );
        assert_eq!(
            std::fs::read(dir.path().join("DIR-Reports/a.txt")).unwrap(),
            b"IN"
        );
    }

    #[tokio::test]
    async fn sync_names_do_not_depend_on_listing_order() {
        let listing = |first: &str, second: &str| {
            json!([
                {"id": first, "name": "a.txt", "mimeType": "text/plain"},
                {"id": second, "name": "a.txt", "mimeType": "text/plain"},
                {"id": "U", "name": "unique.txt", "mimeType": "text/plain"}
            ])
        };
        let mut layouts = Vec::new();
        for (first, second) in [("P", "Q"), ("Q", "P")] {
            let server = MockServer::start().await;
            mount_listing(&server, "ROOT", listing(first, second)).await;
            let api = api(&server.uri(), "drive/v3/");
            let dir = tempfile::tempdir().unwrap();
            sync(&api, "ROOT", dir.path()).await.unwrap();
            let mut names: Vec<String> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            layouts.push(names);
        }
        assert_eq!(
            layouts[0],
            [
                SYNC_STATE_FILE,
                SYNC_LOCK_FILE,
                "P-a.txt",
                "Q-a.txt",
                "unique.txt"
            ]
        );
        assert_eq!(layouts[0], layouts[1]);
    }

    /// `ας.txt` and `ασ.txt` (final and medial sigma) are two Drive names
    /// but one file on APFS and NTFS, which upper-case both to `ΑΣ.TXT`;
    /// `to_lowercase` alone leaves them apart, so the second download
    /// replaced the first. The same holds for `ſ` (long s) and `s`.
    #[test]
    fn disambiguate_merges_names_case_insensitive_disks_merge() {
        for (a, b) in [
            ("\u{3b1}\u{3c2}.txt", "\u{3b1}\u{3c3}.txt"),
            ("\u{17f}.txt", "s.txt"),
        ] {
            let names = disambiguate(&[("A", a), ("B", b)]).unwrap();
            assert_eq!(names, vec![format!("A-{a}"), format!("B-{b}")]);
        }
        let names = disambiguate(&[("A", "a.txt"), ("B", "b.txt")]).unwrap();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[tokio::test]
    async fn sync_rejects_a_prefixed_name_that_collides_with_a_real_one() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "P", "name": "a.txt", "mimeType": "text/plain"},
                {"id": "Q", "name": "a.txt", "mimeType": "text/plain"},
                {"id": "Z", "name": "P-a.txt", "mimeType": "text/plain"}
            ]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let err = sync(&api, "ROOT", dir.path()).await.unwrap_err();
        assert!(err.to_string().contains("P-a.txt"), "{err}");
        // Nothing but the lock file: no Drive item was written.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [SYNC_LOCK_FILE], "{names:?}");
    }

    /// File systems cap a name at 255 bytes, not characters: a long non-ASCII
    /// Drive name (plus the export extension) must still be writable.
    #[tokio::test]
    async fn sync_fits_long_multibyte_names_in_the_file_system_limit() {
        // 150 chars: 300 UTF-16 units (APFS limit 255) and 600 UTF-8 bytes
        // (ext4 limit 255).
        let long_name = "\u{1F4C4}".repeat(150);
        let server = MockServer::start().await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "D", "name": long_name, "mimeType": DOC},
                {"id": "A", "name": "a.txt", "mimeType": "text/plain"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/A"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"A".to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/D/export"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"DOCX".to_vec()))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["downloaded"].as_array().unwrap().len(), 2, "{v}");
        let doc = PathBuf::from(v["downloaded"][0]["path"].as_str().unwrap());
        let file_name = doc.file_name().unwrap().to_str().unwrap();
        assert!(file_name.len() <= 255, "{file_name}");
        assert!(file_name.starts_with('\u{1F4C4}'), "{file_name}");
        assert!(file_name.ends_with(".docx"), "{file_name}");
        assert_eq!(std::fs::read(&doc).unwrap(), b"DOCX");
    }

    /// Long names that collide get an ID prefix; shortening the prefixed name
    /// must keep the extension (here of a download and of an export).
    #[tokio::test]
    async fn sync_keeps_extensions_of_long_colliding_names() {
        let long = "r".repeat(200);
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "P1", "name": format!("{long}.pdf"), "mimeType": "application/pdf"},
                {"id": "P2", "name": format!("{long}.pdf"), "mimeType": "application/pdf"}
            ]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        let names: Vec<String> = v["downloaded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let p = PathBuf::from(d["path"].as_str().unwrap());
                p.file_name().unwrap().to_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(names.len(), 2, "{v}");
        for (name, id) in names.iter().zip(["P1", "P2"]) {
            assert!(name.starts_with(&format!("{id}-r")), "{name}");
            assert!(name.ends_with("r.pdf"), "{name}");
            assert!(name.len() <= 255, "{name}");
        }
    }

    #[test]
    fn a_drive_file_named_dash_is_saved_to_a_file_not_stdout() {
        let target = resolve_target(None, "-", None, false).unwrap();
        assert!(
            matches!(&target, OutputTarget::File { path, .. } if path.file_name().unwrap() == "-"),
            "a Drive name must never select stdout: {target:?}"
        );
        // The user's own `--output -` still streams to stdout.
        assert_eq!(
            resolve_target(Some("-"), "a.pdf", None, false).unwrap(),
            OutputTarget::Stdout
        );
    }

    #[test]
    fn download_default_name_keeps_the_extension_of_a_long_name() {
        let name = format!("{}.pdf", "d".repeat(250));
        let target = resolve_target(None, &name, None, false).unwrap();
        let local = target.describe();
        let local = Path::new(local.as_str().unwrap()).file_name().unwrap();
        assert_eq!(local.to_str().unwrap(), format!("{}.pdf", "d".repeat(196)));
    }

    /// Drive names are case-sensitive but the default macOS and Windows
    /// file systems are not: `Report.txt` and `report.txt` must not share a
    /// local path, or the second one is silently reported "unchanged" (or
    /// overwrites the first).
    #[tokio::test]
    async fn sync_disambiguates_names_differing_only_by_case() {
        let server = MockServer::start().await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "U", "name": "Report.txt", "mimeType": "text/plain", "modifiedTime": "2020-01-01T00:00:00Z"},
                {"id": "L", "name": "report.txt", "mimeType": "text/plain", "modifiedTime": "2020-01-01T00:00:00Z"}
            ]})))
            .mount(&server)
            .await;
        for id in ["U", "L"] {
            Mock::given(method("GET"))
                .and(path(format!("/drive/v3/files/{id}")))
                .and(query_param("alt", "media"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(id.as_bytes().to_vec()))
                .mount(&server)
                .await;
        }
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"], json!([]), "{v}");
        let paths: Vec<PathBuf> = v["downloaded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| PathBuf::from(d["path"].as_str().unwrap()))
            .collect();
        assert_eq!(paths.len(), 2, "{v}");
        let folded: std::collections::HashSet<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_lowercase())
            .collect();
        assert_eq!(folded.len(), 2, "paths collide case-insensitively: {v}");
        let mut contents: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        contents.sort();
        assert_eq!(contents, vec![b"L".to_vec(), b"U".to_vec()]);
    }

    /// Drive keeps names byte-for-byte, but APFS (the macOS default) treats
    /// the NFC and NFD spellings of a name as the same file. `café.txt`
    /// (precomposed) and `cafe\u{301}.txt` (decomposed) must not share a local
    /// path, or the second is silently reported "unchanged" (or overwrites
    /// the first).
    #[tokio::test]
    async fn sync_disambiguates_names_differing_only_by_unicode_normalization() {
        let server = MockServer::start().await;
        mount_root(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [
                {"id": "C", "name": "caf\u{e9}.txt", "mimeType": "text/plain", "modifiedTime": "2020-01-01T00:00:00Z"},
                {"id": "D", "name": "cafe\u{301}.txt", "mimeType": "text/plain", "modifiedTime": "2020-01-01T00:00:00Z"}
            ]})))
            .mount(&server)
            .await;
        for id in ["C", "D"] {
            Mock::given(method("GET"))
                .and(path(format!("/drive/v3/files/{id}")))
                .and(query_param("alt", "media"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(id.as_bytes().to_vec()))
                .mount(&server)
                .await;
        }
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"], json!([]), "{v}");
        let paths: Vec<String> = v["downloaded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["path"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(paths.len(), 2, "{v}");
        let normalized: std::collections::HashSet<String> =
            paths.iter().map(|p| nfc(p).into_owned()).collect();
        assert_eq!(
            normalized.len(),
            2,
            "paths collide after NFC normalization: {v}"
        );
        let mut contents: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        contents.sort();
        assert_eq!(contents, vec![b"C".to_vec(), b"D".to_vec()]);
    }

    #[test]
    fn inject_commands_registers_all() {
        let cmd = DriveHelper.inject_commands(
            Command::new("gwsr"),
            &crate::discovery::RestDescription::default(),
        );
        let names: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        for n in [
            "+upload",
            "+download",
            "+export",
            "+move",
            "+share",
            "+sync",
        ] {
            assert!(names.contains(&n), "{n}");
        }
    }

    /// Mounts a fresh root listing with one plain file `name` (Drive `id`,
    /// fingerprint fields from `extra`) whose download returns `body`.
    async fn mount_one(server: &MockServer, id: &str, name: &str, extra: Value, body: &[u8]) {
        server.reset().await;
        let mut item = json!({"id": id, "name": name, "mimeType": "text/plain"});
        for (k, v) in extra.as_object().unwrap() {
            item[k] = v.clone();
        }
        mount_root(server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'ROOT' in parents and trashed = false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [item]})))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/drive/v3/files/{id}")))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(server)
            .await;
    }

    fn v1() -> Value {
        json!({"modifiedTime": "2024-01-01T00:00:00Z", "version": "1", "size": "2"})
    }

    fn v2() -> Value {
        json!({"modifiedTime": "2024-02-01T00:00:00Z", "version": "2", "size": "2"})
    }

    fn read_state(dir: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(dir.join(SYNC_STATE_FILE)).unwrap()).unwrap()
    }

    fn ids(v: &Value, key: &str) -> Vec<String> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect()
    }

    /// Changes the local file and moves its mtime off the recorded one, as an
    /// editor would.
    fn edit_locally(p: &Path, content: &[u8]) {
        std::fs::write(p, content).unwrap();
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
    }

    #[tokio::test]
    async fn sync_first_run_downloads_and_records_state_then_is_unchanged() {
        let server = MockServer::start().await;
        mount_one(&server, "A", "a.txt", v1(), b"v1").await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["A"], "{v}");
        let state = read_state(dir.path());
        assert_eq!(state["version"], SYNC_STATE_VERSION);
        assert_eq!(state["folderId"], "ROOT");
        let rec = &state["files"]["A"];
        assert_eq!(rec["path"], "a.txt");
        assert_eq!(rec["remote"]["version"], "1");
        assert_eq!(rec["remote"]["modifiedTime"], "2024-01-01T00:00:00Z");
        assert_eq!(rec["local"]["size"], 2);
        // SHA-256 of "v1".
        assert_eq!(
            rec["local"]["sha256"],
            "3bfc269594ef649228e9a74bab00f042efc91d5acc6fbee31a382e80d42388fe"
        );

        // Nothing changed: no download (the server would serve it again,
        // but the report must say unchanged).
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert!(ids(&v, "downloaded").is_empty(), "{v}");
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 1, "{v}");
        assert!(sync_failure(&v).is_none());
        // The state and lock files are never mirrored or reported as a Drive item.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[tokio::test]
    async fn sync_updates_a_file_changed_in_drive_and_untouched_locally() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        mount_one(&server, "A", "a.txt", v1(), b"v1").await;
        sync(&api, "ROOT", dir.path()).await.unwrap();
        // A `touch` alone is not an edit: the content still matches.
        let p = dir.path().join("a.txt");
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();

        mount_one(&server, "A", "a.txt", v2(), b"v2").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["A"], "{v}");
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        assert_eq!(
            read_state(dir.path())["files"]["A"]["remote"]["version"],
            "2"
        );
        assert!(sync_failure(&v).is_none());
    }

    #[tokio::test]
    async fn sync_keeps_a_locally_edited_file_and_reports_a_conflict() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        mount_one(&server, "A", "a.txt", v1(), b"v1").await;
        sync(&api, "ROOT", dir.path()).await.unwrap();
        edit_locally(&p, b"mine");

        // Drive unchanged: kept, reported.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "conflicts"), ["A"], "{v}");
        assert_eq!(v["conflicts"][0]["reason"], "modifiedLocally");
        assert_eq!(v["conflicts"][0]["remoteChanged"], false);

        // Drive changed too: still kept, and the run fails loudly.
        mount_one(&server, "A", "a.txt", v2(), b"v2").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"mine");
        assert!(ids(&v, "downloaded").is_empty(), "{v}");
        assert_eq!(v["conflicts"][0]["remoteChanged"], true);
        let err = sync_failure(&v).unwrap();
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
        assert_eq!(err.to_json()["error"]["reason"], "syncConflict");
        assert!(err.to_string().contains("--overwrite"), "{err}");
        // The record still describes the last synced version.
        assert_eq!(
            read_state(dir.path())["files"]["A"]["remote"]["version"],
            "1"
        );
    }

    #[tokio::test]
    async fn sync_overwrite_replaces_a_locally_edited_file() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        mount_one(&server, "A", "a.txt", v1(), b"v1").await;
        sync(&api, "ROOT", dir.path()).await.unwrap();
        edit_locally(&p, b"mine");
        mount_one(&server, "A", "a.txt", v2(), b"v2").await;

        let v = sync_uninterrupted(&api, "ROOT", dir.path(), true)
            .await
            .unwrap();
        assert_eq!(ids(&v, "downloaded"), ["A"], "{v}");
        assert!(ids(&v, "conflicts").is_empty(), "{v}");
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        assert!(sync_failure(&v).is_none());
        // Back in sync: the next plain run has nothing to do.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 1, "{v}");
    }

    /// The bug PR #33 started from: `budget.xlsx` is trashed in Drive and a
    /// different file with the same name and an *older* `modifiedTime` is
    /// moved in. It must replace the pristine mirror copy, and must not
    /// replace an edited one.
    #[tokio::test]
    async fn sync_replaces_a_file_that_drive_replaced_under_the_same_name() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let older = json!({"modifiedTime": "2023-06-01T00:00:00Z", "version": "7", "size": "12"});

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("budget.xlsx");
        mount_one(&server, "A", "budget.xlsx", v1(), b"old-A").await;
        sync(&api, "ROOT", dir.path()).await.unwrap();
        mount_one(&server, "B", "budget.xlsx", older.clone(), b"new-B-12byte").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["B"], "{v}");
        assert_eq!(std::fs::read(&p).unwrap(), b"new-B-12byte");
        let state = read_state(dir.path());
        assert!(state["files"].get("A").is_none(), "{state}");
        assert_eq!(state["files"]["B"]["path"], "budget.xlsx");

        // Same replacement over a locally edited copy: kept.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("budget.xlsx");
        mount_one(&server, "A", "budget.xlsx", v1(), b"old-A").await;
        sync(&api, "ROOT", dir.path()).await.unwrap();
        edit_locally(&p, b"my numbers");
        mount_one(&server, "B", "budget.xlsx", older, b"new-B-12byte").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "conflicts"), ["B"], "{v}");
        assert_eq!(v["conflicts"][0]["remoteChanged"], true);
        assert_eq!(std::fs::read(&p).unwrap(), b"my numbers");
    }

    /// A renamed in Drive (x.txt -> y.txt) and B moved in as x.txt: B
    /// replaces A's pristine copy whichever of the two is listed first.
    #[tokio::test]
    async fn sync_handles_a_rename_and_a_takeover_in_any_order() {
        for order in [["A", "B"], ["B", "A"]] {
            let server = MockServer::start().await;
            let api = api(&server.uri(), "drive/v3/");
            let dir = tempfile::tempdir().unwrap();
            mount_listing(
                &server,
                "ROOT",
                json!([{"id": "A", "name": "x.txt", "mimeType": "text/plain",
                        "modifiedTime": "2024-01-01T00:00:00Z"}]),
            )
            .await;
            sync(&api, "ROOT", dir.path()).await.unwrap();
            server.reset().await;
            let item = |id: &str| {
                let name = if id == "A" { "y.txt" } else { "x.txt" };
                json!({"id": id, "name": name, "mimeType": "text/plain",
                       "modifiedTime": "2024-01-01T00:00:00Z"})
            };
            mount_listing(&server, "ROOT", json!([item(order[0]), item(order[1])])).await;
            let v = sync(&api, "ROOT", dir.path()).await.unwrap();
            assert!(ids(&v, "conflicts").is_empty(), "{order:?}: {v}");
            assert_eq!(std::fs::read(dir.path().join("x.txt")).unwrap(), b"B");
            assert_eq!(std::fs::read(dir.path().join("y.txt")).unwrap(), b"A");
            let state = read_state(dir.path());
            assert_eq!(state["files"]["A"]["path"], "y.txt", "{state}");
            assert_eq!(state["files"]["B"]["path"], "x.txt", "{state}");
        }
    }

    /// Two runs on one directory would each load the state, then the one
    /// saving last would drop what the other recorded. A second run is
    /// refused while the first holds the directory, before it downloads or
    /// writes anything.
    #[tokio::test]
    async fn sync_refuses_a_directory_another_sync_is_using() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        mount_one(&server, "A", "a.txt", v1(), b"v1").await;
        let held = crate::fs_util::FileLock::acquire(
            &dir.path().join(SYNC_LOCK_FILE),
            std::time::Duration::from_secs(1),
        )
        .unwrap();
        let err = sync(&api, "ROOT", dir.path()).await.unwrap_err();
        assert!(err.to_string().contains("another"), "{err}");
        assert!(!dir.path().join("a.txt").exists());
        assert!(!dir.path().join(SYNC_STATE_FILE).exists());
        // Only the read-only check that the folder exists ran.
        let requests = server.received_requests().await.unwrap();
        let paths: Vec<_> = requests.iter().map(|r| r.url.path().to_string()).collect();
        assert_eq!(paths, ["/drive/v3/files/ROOT"], "{paths:?}");
        drop(held);
        // Released: the next run goes ahead.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["A"], "{v}");
    }

    /// A file with two parents inside the synced tree is listed in both
    /// folders under one ID. The state keeps one path per ID, so the copy at
    /// the other path must still count as written by the sync: not as an
    /// untracked conflict on every later run, and updated when Drive changes.
    #[tokio::test]
    async fn sync_tracks_a_file_listed_in_two_synced_folders() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let mount = |modified: &'static str, body: &'static [u8]| {
            let server = &server;
            async move {
                server.reset().await;
                let doc = json!({"id": "D", "name": "notes", "mimeType": DOC,
                                 "modifiedTime": modified, "version": "1"});
                mount_listing(
                    server,
                    "ROOT",
                    json!([doc.clone(), {"id": "S", "name": "sub", "mimeType": FOLDER_MIME}]),
                )
                .await;
                mount_listing(server, "S", json!([doc])).await;
                Mock::given(method("GET"))
                    .and(path("/drive/v3/files/D/export"))
                    .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
                    .mount(server)
                    .await;
            }
        };
        let top = dir.path().join("notes.docx");
        let nested = dir.path().join("sub").join("notes.docx");

        mount("2024-01-01T00:00:00Z", b"v1").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["D", "D"], "{v}");

        // Nothing changed: nothing to do, and no conflict.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert!(ids(&v, "conflicts").is_empty(), "{v}");
        assert!(sync_failure(&v).is_none(), "{v}");
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 2, "{v}");

        // Changed in Drive: both pristine copies are updated.
        mount("2024-02-01T00:00:00Z", b"v2").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert!(ids(&v, "conflicts").is_empty(), "{v}");
        assert_eq!(ids(&v, "downloaded"), ["D", "D"], "{v}");
        assert_eq!(std::fs::read(&top).unwrap(), b"v2");
        assert_eq!(std::fs::read(&nested).unwrap(), b"v2");

        // A copy edited locally is still kept and reported.
        edit_locally(&top, b"mine");
        mount("2024-03-01T00:00:00Z", b"v3").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "conflicts"), ["D"], "{v}");
        assert_eq!(std::fs::read(&top).unwrap(), b"mine");
        assert_eq!(std::fs::read(&nested).unwrap(), b"v3");
    }

    #[tokio::test]
    async fn sync_with_a_corrupt_state_never_overwrites_existing_files() {
        let server = MockServer::start().await;
        let api = api(&server.uri(), "drive/v3/");
        for garbage in [
            &b"{not json"[..],
            br#"{"version": 99, "folderId": "ROOT", "files": {}}"#,
            br#"{"version": 1, "folderId": "OTHER", "files": {}}"#,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.txt");
            mount_one(&server, "A", "a.txt", v1(), b"v1").await;
            sync(&api, "ROOT", dir.path()).await.unwrap();
            edit_locally(&p, b"mine");
            std::fs::write(dir.path().join(SYNC_STATE_FILE), garbage).unwrap();

            mount_one(&server, "A", "a.txt", v2(), b"v2").await;
            let v = sync(&api, "ROOT", dir.path()).await.unwrap();
            assert_eq!(ids(&v, "conflicts"), ["A"], "{v}");
            assert_eq!(v["conflicts"][0]["reason"], "untracked");
            assert_eq!(std::fs::read(&p).unwrap(), b"mine");
            assert_eq!(
                sync_failure(&v).unwrap().to_json()["error"]["reason"],
                "syncConflict"
            );
            // The state is rewritten as valid (empty) state.
            let state = read_state(dir.path());
            assert_eq!(state["version"], SYNC_STATE_VERSION, "{state}");
            assert_eq!(state["folderId"], "ROOT");
        }

        // An untracked file Drive's checksum proves identical is adopted.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"v1").unwrap();
        std::fs::write(dir.path().join(SYNC_STATE_FILE), b"\0\0").unwrap();
        let sha = "3bfc269594ef649228e9a74bab00f042efc91d5acc6fbee31a382e80d42388fe";
        mount_one(&server, "A", "a.txt", json!({"sha256Checksum": sha}), b"v1").await;
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(read_state(dir.path())["files"]["A"]["local"]["sha256"], sha);
    }

    #[tokio::test]
    async fn sync_saves_state_for_files_mirrored_before_an_abort() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([
                {"id": "A", "name": "a.txt", "mimeType": "text/plain", "modifiedTime": "2024-01-01T00:00:00Z"},
                {"id": "B", "name": "b.txt", "mimeType": "text/plain"}
            ]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("b.txt")).unwrap();
        assert!(sync(&api, "ROOT", dir.path()).await.is_err());
        assert_eq!(read_state(dir.path())["files"]["A"]["path"], "a.txt");
    }

    #[tokio::test]
    async fn sync_never_maps_a_drive_item_onto_the_state_file() {
        let server = MockServer::start().await;
        mount_listing(
            &server,
            "ROOT",
            json!([{"id": "S", "name": SYNC_STATE_FILE, "mimeType": "text/plain"}]),
        )
        .await;
        let api = api(&server.uri(), "drive/v3/");
        let dir = tempfile::tempdir().unwrap();
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(ids(&v, "downloaded"), ["S"], "{v}");
        assert_ne!(
            v["downloaded"][0]["path"].as_str().unwrap(),
            dir.path().join(SYNC_STATE_FILE).display().to_string()
        );
        assert_eq!(
            read_state(dir.path())["files"]["S"]["path"],
            "gwsr-sync.json"
        );
    }

    #[test]
    fn remote_fingerprint_prefers_checksums_and_needs_a_modified_time() {
        let a = RemoteFingerprint::of(&json!({"modifiedTime": "t1", "version": "1", "size": 3}));
        assert_eq!(a.size.as_deref(), Some("3"));
        assert!(a.same_content(&a.clone()));
        let renamed = RemoteFingerprint::of(&json!({"modifiedTime": "t1", "version": "2"}));
        assert!(!renamed.same_content(&a));
        // Nothing to go on: always fetched again.
        let blank = RemoteFingerprint::default();
        assert!(!blank.same_content(&blank.clone()));
        // A metadata-only change keeps the checksum: same content.
        let c1 = RemoteFingerprint::of(
            &json!({"modifiedTime": "t1", "version": "1", "md5Checksum": "m"}),
        );
        let c2 = RemoteFingerprint::of(
            &json!({"modifiedTime": "t2", "version": "5", "md5Checksum": "m"}),
        );
        assert!(c2.same_content(&c1));
    }

    #[cfg(unix)]
    #[test]
    fn sync_plan_surfaces_an_uninspectable_local_file() {
        // A path through a regular file fails with NotADirectory, not NotFound.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let state = SyncState::load(dir.path(), "ROOT").unwrap();
        let err = plan_sync(
            &state,
            "A",
            &RemoteFingerprint::default(),
            &file.join("child"),
            "f/child",
        )
        .unwrap_err();
        assert!(err.to_string().contains("cannot inspect"), "{err}");
    }
}
