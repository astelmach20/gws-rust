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

//! Slides helpers: `+create`, `+read` (slide text and speaker notes).

use super::Helper;
use super::http::{self, Api, ApiRequest, encode_segment, required};
use crate::error::GwsError;
use clap::{Arg, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_PRESENTATIONS: &str = "https://www.googleapis.com/auth/presentations";
const SCOPE_PRESENTATIONS_READONLY: &str = "https://www.googleapis.com/auth/presentations.readonly";

pub struct SlidesHelper;

impl Helper for SlidesHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+create")
                .about("[Helper] Create a new presentation")
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Presentation title")
                        .required(true)
                        .value_name("TITLE"),
                )
                .after_help("EXAMPLES:\n  gwsr slides +create --title 'Q3 review'\n\nTIPS:\n  Prints presentationId and url."),
        )
        .subcommand(
            Command::new("+read")
                .about("[Helper] Extract the text and speaker notes of every slide")
                .arg(
                    Arg::new("presentation-id")
                        .long("presentation-id")
                        .help("Presentation ID")
                        .required(true)
                        .value_name("ID"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr slides +read --presentation-id PRES_ID
  gwsr slides +read --presentation-id PRES_ID --format yaml

TIPS:
  Read-only. Text from shapes, tables and groups is returned per slide,
  in page order, with the speaker notes.",
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
            let dry = http::dry_run(m);
            let (api, value) = match name {
                "+create" => {
                    let api = Api::new(doc, &[SCOPE_PRESENTATIONS], dry, sanitize).await?;
                    let title = required(m, "title")?;
                    let resp = api
                        .send(
                            ApiRequest::post(api.url("v1/presentations"))
                                .json(json!({ "title": title })),
                        )
                        .await?;
                    let id = resp.get("presentationId").and_then(Value::as_str);
                    let v = if api.is_dry_run() {
                        Value::Null
                    } else {
                        let id = id.ok_or_else(|| {
                            http::other_err(anyhow::anyhow!(
                                "presentations.create returned no presentationId"
                            ))
                        })?;
                        json!({
                            "presentationId": id,
                            "title": title,
                            "url": format!("https://docs.google.com/presentation/d/{id}/edit"),
                        })
                    };
                    (api, v)
                }
                "+read" => {
                    let api = Api::new(doc, &[SCOPE_PRESENTATIONS_READONLY], dry, sanitize).await?;
                    let id = required(m, "presentation-id")?;
                    let pres = api
                        .send(ApiRequest::get(
                            api.url(&format!("v1/presentations/{}", encode_segment(id))),
                        ))
                        .await?;
                    let v = if api.is_dry_run() {
                        Value::Null
                    } else {
                        extract(&pres)
                    };
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

fn shape_text(shape: &Value) -> String {
    shape
        .pointer("/text/textElements")
        .and_then(Value::as_array)
        .map(|els| {
            els.iter()
                .filter_map(|e| e.pointer("/textRun/content").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// Collect text from page elements, recursing into groups and tables.
fn collect(elements: &[Value], out: &mut Vec<String>) {
    for el in elements {
        if let Some(shape) = el.get("shape") {
            let t = shape_text(shape);
            if !t.trim().is_empty() {
                out.push(t.trim_end().to_string());
            }
        } else if let Some(children) = el
            .pointer("/elementGroup/children")
            .and_then(Value::as_array)
        {
            collect(children, out);
        } else if let Some(rows) = el.pointer("/table/tableRows").and_then(Value::as_array) {
            for row in rows {
                let cells: Vec<String> = row
                    .get("tableCells")
                    .and_then(Value::as_array)
                    .map(|cells| {
                        cells
                            .iter()
                            .map(|c| shape_text(c).trim().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                if cells.iter().any(|c| !c.is_empty()) {
                    out.push(cells.join("\t"));
                }
            }
        }
    }
}

fn extract(pres: &Value) -> Value {
    let slides: Vec<Value> = pres
        .get("slides")
        .and_then(Value::as_array)
        .map(|slides| {
            slides
                .iter()
                .enumerate()
                .map(|(i, slide)| {
                    let mut text = Vec::new();
                    collect(
                        slide
                            .get("pageElements")
                            .and_then(Value::as_array)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                        &mut text,
                    );
                    let notes_id = slide
                        .pointer("/slideProperties/notesPage/notesProperties/speakerNotesObjectId")
                        .and_then(Value::as_str);
                    let notes = slide
                        .pointer("/slideProperties/notesPage/pageElements")
                        .and_then(Value::as_array)
                        .and_then(|els| {
                            els.iter()
                                .find(|e| e.get("objectId").and_then(Value::as_str) == notes_id)
                        })
                        .and_then(|e| e.get("shape"))
                        .map(|s| shape_text(s).trim().to_string())
                        .unwrap_or_default();
                    json!({
                        "index": i + 1,
                        "objectId": slide.get("objectId"),
                        "text": text.join("\n"),
                        "notes": notes,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "presentationId": pres.get("presentationId"),
        "title": pres.get("title"),
        "slides": slides,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_tables_groups_and_notes() {
        let run = |t: &str| json!({"text": {"textElements": [{"textRun": {"content": t}}]}});
        let pres = json!({
            "presentationId": "P", "title": "Deck",
            "slides": [{
                "objectId": "s1",
                "pageElements": [
                    {"shape": run("Title\n")},
                    {"elementGroup": {"children": [{"shape": run("Grouped\n")}]}},
                    {"table": {"tableRows": [{"tableCells": [run("A"), run("B")]}]}}
                ],
                "slideProperties": {"notesPage": {
                    "notesProperties": {"speakerNotesObjectId": "n1"},
                    "pageElements": [{"objectId": "n1", "shape": run("Say hi\n")}]
                }}
            }]
        });
        let v = extract(&pres);
        assert_eq!(v["slides"][0]["text"], "Title\nGrouped\nA\tB");
        assert_eq!(v["slides"][0]["notes"], "Say hi");
        assert_eq!(v["slides"][0]["index"], 1);
    }
}
