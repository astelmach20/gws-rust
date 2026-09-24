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
use super::http::{self, Api, ApiRequest, OutputTarget, safe_filename};
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
const FILE_FIELDS: &str = "id,name,mimeType,parents,size,modifiedTime,webViewLink,driveId";

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
                .after_help(
                    "\
EXAMPLES:
  gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror
  gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --dry-run

TIPS:
  Recurses into sub-folders. Regular files are downloaded; Google
  Docs/Sheets/Slides/Drawings are exported as docx/xlsx/pptx/pdf.
  Local files are replaced only when the Drive copy is newer; local files
  that are not in Drive are left untouched (nothing is ever deleted).
  Other Google-native types (Forms, Sites, shortcuts...) are reported as skipped.",
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
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let v = download(
                        &api,
                        required(m, "file-id")?,
                        optional(m, "output")?,
                        flag(m, "overwrite")?,
                    )
                    .await?;
                    (api, v)
                }
                "+export" => {
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let v = export(
                        &api,
                        required(m, "file-id")?,
                        required(m, "to")?,
                        optional(m, "output")?,
                        flag(m, "overwrite")?,
                    )
                    .await?;
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
                    let api = Api::new(doc, &[SCOPE_DRIVE_READONLY], dry, sanitize).await?;
                    let v = sync(&api, required(m, "folder-id")?, &dir).await?;
                    (api, v)
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
            Some(t) => t.to_string(),
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

/// Google-native type to convert an uploaded file into, by source MIME type.
fn conversion_target(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/msword"
        | "application/vnd.oasis.opendocument.text"
        | "application/rtf"
        | "text/rtf"
        | "text/plain"
        | "text/markdown"
        | "text/html" => "application/vnd.google-apps.document",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet"
        | "text/csv"
        | "text/tab-separated-values" => "application/vnd.google-apps.spreadsheet",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        | "application/vnd.ms-powerpoint"
        | "application/vnd.oasis.opendocument.presentation" => {
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

fn resolve_target(
    output: Option<&str>,
    default_name: &str,
    overwrite: bool,
) -> Result<OutputTarget, GwsError> {
    match output {
        Some(o) => OutputTarget::parse(o, overwrite),
        None => OutputTarget::parse(&safe_filename(default_name, "download"), overwrite),
    }
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
            resolve_target(output, "download", overwrite)?,
        )
    } else {
        let meta = get_metadata(api, file_id).await?;
        let mime = str_field(&meta, "mimeType")?;
        if mime.starts_with("application/vnd.google-apps.") {
            return Err(GwsError::Validation(format!(
                "'{}' is a Google-native file ({mime}) with no binary content; use `gwsr drive +export --file-id {file_id} --to <format>`",
                str_field(&meta, "name")?
            )));
        }
        let name = str_field(&meta, "name")?.to_string();
        let target = resolve_target(output, &name, overwrite)?;
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
        return Err(GwsError::Validation(format!(
            "Files of type {source_mime} cannot be exported; use +download for regular files"
        )));
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
        check_export(format, str_field(&meta, "mimeType")?)?;
        str_field(&meta, "name")?.to_string()
    };
    let target = resolve_target(output, &with_extension(&name, format.ext), overwrite)?;
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
        Ok(Self {
            file_id: required(m, "file-id")?.to_string(),
            role: required(m, "role")?.to_string(),
            grantee,
            notify: !flag(m, "no-notify")?,
            message: optional(m, "message")?.map(str::to_string),
        })
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

    fn body(&self) -> Result<Value, GwsError> {
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
        if self.role == "owner" && !matches!(self.grantee, Grantee::User(_)) {
            return Err(GwsError::Validation(
                "--role owner can only be granted to a single user (--email without --group)"
                    .into(),
            ));
        }
        Ok(body)
    }
}

async fn share(api: &Api, args: &ShareArgs) -> Result<Value, GwsError> {
    let body = args.body()?;
    let is_person = matches!(args.grantee, Grantee::User(_) | Grantee::Group(_));
    let mut req = ApiRequest::post(api.url(&format!(
        "files/{}/permissions",
        encode_path_segment(&args.file_id)
    )))
    .query("supportsAllDrives", "true")
    .json(body);
    if is_person {
        req = req.query("sendNotificationEmail", args.notify.to_string());
        if let Some(msg) = &args.message {
            req = req.query("emailMessage", msg.clone());
        }
    } else if args.message.is_some() {
        return Err(GwsError::Validation(
            "--message only applies to --email grants".into(),
        ));
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
                    "nextPageToken,files(id,name,mimeType,modifiedTime,size)",
                ),
            "files",
            None,
        )
        .await?;
    Ok(page.items)
}

/// Whether the local copy is at least as new as the Drive item. A missing
/// local file or a missing/unparseable remote `modifiedTime` means "download
/// it" (the safe answer); a local file that exists but cannot be inspected is
/// an error rather than a silent re-download over it.
fn is_up_to_date(local: &Path, remote_modified: Option<&str>) -> Result<bool, GwsError> {
    let Some(remote) = remote_modified.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    else {
        return Ok(false);
    };
    let meta = match std::fs::metadata(local) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(GwsError::other(anyhow::anyhow!(
                "cannot inspect '{}': {e}",
                local.display()
            )));
        }
    };
    let modified = meta.modified().map_err(|e| {
        GwsError::other(anyhow::anyhow!(
            "cannot read the modification time of '{}': {e}",
            local.display()
        ))
    })?;
    Ok(chrono::DateTime::<chrono::Utc>::from(modified) >= remote.with_timezone(&chrono::Utc))
}

async fn sync(api: &Api, folder_id: &str, dir: &Path) -> Result<Value, GwsError> {
    let mut downloaded = Vec::new();
    let mut unchanged = Vec::new();
    let mut skipped = Vec::new();
    let mut stack: Vec<(String, PathBuf)> = vec![(folder_id.to_string(), dir.to_path_buf())];
    let mut visited = std::collections::HashSet::new();

    if api.is_dry_run() {
        list_children(api, folder_id).await?;
        api.plan_note(json!({
            "note": "each child file is downloaded (or exported) into the directory; sub-folders are recursed",
            "dir": dir.display().to_string(),
        }))?;
        return Ok(Value::Null);
    }

    while let Some((id, local_dir)) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let mut used_names = std::collections::HashSet::new();
        for child in list_children(api, &id).await? {
            let child_id = str_field(&child, "id")?;
            let name = str_field(&child, "name")?;
            let mime = str_field(&child, "mimeType")?;
            let modified = child.get("modifiedTime").and_then(Value::as_str);
            let mut local_name = safe_filename(name, child_id);
            if mime == FOLDER_MIME {
                stack.push((child_id.to_string(), local_dir.join(&local_name)));
                continue;
            }
            let export = if mime.starts_with("application/vnd.google-apps.") {
                match sync_export(mime) {
                    Some(f) => {
                        local_name = with_extension(&local_name, f.ext);
                        Some(f)
                    }
                    None => {
                        skipped.push(json!({"id": child_id, "name": name, "mimeType": mime, "reason": "Google-native type with no export"}));
                        continue;
                    }
                }
            } else {
                None
            };
            // Two Drive items can share a name; disambiguate with the ID.
            if !used_names.insert(local_name.clone()) {
                local_name = format!("{child_id}-{local_name}");
                used_names.insert(local_name.clone());
            }
            let path = local_dir.join(&local_name);
            if is_up_to_date(&path, modified)? {
                unchanged.push(path.display().to_string());
                continue;
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
            let bytes = api.download(req, &target).await?;
            downloaded
                .push(json!({"id": child_id, "path": path.display().to_string(), "bytes": bytes}));
        }
    }
    Ok(json!({
        "downloaded": downloaded,
        "unchanged": unchanged,
        "skipped": skipped,
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

        // Second run: local copies are newer than the 2020 remote timestamps.
        let v = sync(&api, "ROOT", dir.path()).await.unwrap();
        assert_eq!(v["unchanged"].as_array().unwrap().len(), 2);
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

    #[test]
    fn up_to_date_compares_mtimes_and_treats_missing_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        assert!(!is_up_to_date(&p, Some("2000-01-01T00:00:00Z")).unwrap());
        std::fs::write(&p, b"x").unwrap();
        assert!(is_up_to_date(&p, Some("2000-01-01T00:00:00Z")).unwrap());
        assert!(!is_up_to_date(&p, Some("2999-01-01T00:00:00Z")).unwrap());
        assert!(!is_up_to_date(&p, None).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn up_to_date_surfaces_an_uninspectable_local_file() {
        // A path through a regular file fails with NotADirectory, not NotFound.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let err = is_up_to_date(&file.join("child"), Some("2000-01-01T00:00:00Z")).unwrap_err();
        assert!(err.to_string().contains("cannot inspect"), "{err}");
    }
}
