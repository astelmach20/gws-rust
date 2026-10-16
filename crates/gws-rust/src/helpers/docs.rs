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

//! Docs helpers: `+create`, `+read`, `+write`, `+replace`.

mod markdown;
mod reader;

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, optional, required};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_DOCS: &str = "https://www.googleapis.com/auth/documents";
const SCOPE_DOCS_READONLY: &str = "https://www.googleapis.com/auth/documents.readonly";

pub struct DocsHelper;

fn document_id_arg() -> Arg {
    Arg::new("document-id")
        .long("document-id")
        .help("Document ID")
        .required(true)
        .value_name("ID")
}

/// `--text` / `--text-file` / `--markdown` content arguments.
fn content_args(cmd: Command, required: bool) -> Command {
    cmd.arg(
        Arg::new("text")
            .long("text")
            .help("Content to add")
            // Content may start with '-': a Markdown list item, a rule.
            .allow_hyphen_values(true)
            .value_name("TEXT"),
    )
    .arg(
        Arg::new("text-file")
            .long("text-file")
            .help("Read the content from a file, or '-' for stdin")
            .value_name("PATH"),
    )
    .group(
        ArgGroup::new("content")
            .args(["text", "text-file"])
            .required(required),
    )
    .arg(
        Arg::new("markdown")
            .long("markdown")
            .help("Interpret the content as Markdown and convert it to native Docs formatting")
            .action(ArgAction::SetTrue),
    )
}

impl Helper for DocsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(content_args(
            Command::new("+create")
                .about("[Helper] Create a new document, optionally with content")
                .arg(
                    Arg::new("title")
                        .long("title")
                        .allow_hyphen_values(true)
                        .help("Document title")
                        .required(true)
                        .value_name("TITLE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr docs +create --title 'Meeting notes'
  gwsr docs +create --title 'Post-mortem' --markdown --text-file ./postmortem.md

TIPS:
  Prints the new document (documentId, title, url).",
                ),
            false,
        ))
        .subcommand(
            Command::new("+read")
                .about("[Helper] Read a document as plain text or Markdown")
                .arg(document_id_arg())
                .arg(
                    Arg::new("body-format")
                        .long("body-format")
                        .help("How to render the body: markdown, text, or raw (the Docs API document JSON)")
                        .value_parser(["markdown", "text", "raw"])
                        .default_value("markdown")
                        .value_name("FORMAT"),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Write the rendered body to this file, or '-' for raw text on stdout")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::new("overwrite")
                        .long("overwrite")
                        .help("Replace an existing --output file")
                        .requires("output")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr docs +read --document-id DOC_ID
  gwsr docs +read --document-id DOC_ID --body-format text --output doc.txt
  gwsr docs +read --document-id DOC_ID --output - | less

TIPS:
  Read-only. Prints JSON {documentId, title, bodyFormat, body} by default.
  Headings, lists, bold/italic/strikethrough, inline code, links and tables
  are rendered; images appear as [image] placeholders.
  Only the first tab of multi-tab documents is read.",
                ),
        )
        .subcommand(content_args(
            Command::new("+write")
                .about("[Helper] Append text or Markdown to the end of a document")
                .arg(document_id_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr docs +write --document-id DOC_ID --text 'Hello, world!'
  gwsr docs +write --document-id DOC_ID --markdown --text '# Title

Some **bold** text and a [link](https://example.com).

- item one
- item two'
  cat notes.md | gwsr docs +write --document-id DOC_ID --markdown --text-file -

TIPS:
  Without --markdown the text is appended verbatim to the last paragraph;
  start it with a newline to begin a new paragraph.
  With --markdown the content starts on a new paragraph and headings,
  bold/italic/strikethrough, inline and fenced code, links, block quotes,
  horizontal rules and (nested) bullet/numbered lists become native Docs
  formatting. Tables, images and raw HTML are rejected with an error.",
                ),
            true,
        ))
        .subcommand(
            Command::new("+replace")
                .about("[Helper] Find and replace text throughout a document")
                .arg(document_id_arg())
                .arg(
                    Arg::new("find")
                        .long("find")
                        .help("Text to find")
                        .required(true)
                        .allow_hyphen_values(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("replace-with")
                        .long("replace-with")
                        .help("Replacement text (may be empty to delete matches)")
                        .required(true)
                        .allow_hyphen_values(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("match-case")
                        .long("match-case")
                        .help("Case-sensitive matching")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr docs +replace --document-id DOC_ID --find '{{name}}' --replace-with 'Alice'
  gwsr docs +replace --document-id DOC_ID --find 'DRAFT' --replace-with '' --match-case

TIPS:
  Prints occurrencesChanged; 0 means nothing matched.",
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
            match name {
                "+create" => {
                    let content = Content::parse(m)?;
                    let api = Api::new(doc, &[SCOPE_DOCS], dry, sanitize).await?;
                    let v = create(&api, required(m, "title")?, content.as_ref()).await?;
                    api.emit(m, &v).await?;
                }
                "+read" => {
                    let format = required(m, "body-format")?;
                    let target = optional(m, "output")?
                        .map(|o| http::OutputTarget::parse(o, flag(m, "overwrite")?))
                        .transpose()?;
                    if format == "raw" && target.is_some() {
                        return Err(GwsError::Validation(
                            "--output needs --body-format markdown or text".into(),
                        ));
                    }
                    let api = Api::new(doc, &[SCOPE_DOCS_READONLY], dry, sanitize).await?;
                    let id = required(m, "document-id")?;
                    let document = get_document(&api, id).await?;
                    if api.is_dry_run() || format == "raw" {
                        api.emit(m, &document).await?;
                        return Ok(true);
                    }
                    let body = reader::render(&document, format == "markdown");
                    let body = body.trim_end_matches('\n');
                    match target {
                        None => {
                            let v = json!({
                                "documentId": id,
                                "title": document.get("title"),
                                "bodyFormat": format,
                                "body": body,
                            });
                            api.emit(m, &v).await?;
                        }
                        Some(http::OutputTarget::Stdout) => api.emit_text(m, body).await?,
                        Some(http::OutputTarget::File { path, overwrite }) => {
                            let mut data = body.to_string();
                            data.push('\n');
                            api.screen_text(&data).await?;
                            crate::output_file::write_atomic(&path, data.as_bytes(), overwrite)
                                .await?;
                            let v = json!({"documentId": id, "output": path.display().to_string(), "bytes": data.len()});
                            api.emit(m, &v).await?;
                        }
                    }
                }
                "+write" => {
                    let content = Content::parse(m)?.ok_or_else(|| {
                        GwsError::Validation("--text or --text-file is required".into())
                    })?;
                    let api = Api::new(doc, &[SCOPE_DOCS], dry, sanitize).await?;
                    let v = write(&api, required(m, "document-id")?, &content).await?;
                    api.emit(m, &v).await?;
                }
                "+replace" => {
                    check_find(required(m, "find")?)?;
                    let api = Api::new(doc, &[SCOPE_DOCS], dry, sanitize).await?;
                    let v = replace(
                        &api,
                        required(m, "document-id")?,
                        required(m, "find")?,
                        required(m, "replace-with")?,
                        flag(m, "match-case")?,
                    )
                    .await?;
                    api.emit(m, &v).await?;
                }
                _ => return Ok(false),
            }
            Ok(true)
        })
    }
}

/// Content to insert.
#[derive(Debug, Clone, PartialEq)]
struct Content {
    text: String,
    markdown: bool,
}

impl Content {
    fn parse(m: &ArgMatches) -> Result<Option<Self>, GwsError> {
        let text = match (optional(m, "text")?, optional(m, "text-file")?) {
            (Some(t), _) => t.to_string(),
            (None, Some(path)) => http::read_text_input(path, "--text-file")?,
            (None, None) => {
                if flag(m, "markdown")? {
                    return Err(GwsError::Validation(
                        "--markdown needs --text or --text-file".into(),
                    ));
                }
                return Ok(None);
            }
        };
        if text.is_empty() {
            return Err(GwsError::Validation("Content is empty".into()));
        }
        let markdown = flag(m, "markdown")?;
        if markdown {
            // Reject unsupported Markdown before credentials are loaded and,
            // for +create, before the document is created.
            markdown::render(&text)?;
        }
        Ok(Some(Self { text, markdown }))
    }
}

async fn get_document(api: &Api, document_id: &str) -> Result<Value, GwsError> {
    api.send(ApiRequest::get(api.url(&format!(
        "v1/documents/{}",
        encode_path_segment(document_id)
    ))))
    .await
}

fn batch_update_request(api: &Api, document_id: &str, requests: Vec<Value>) -> ApiRequest {
    ApiRequest::post(api.url(&format!(
        "v1/documents/{}:batchUpdate",
        encode_path_segment(document_id)
    )))
    .json(json!({ "requests": requests }))
}

/// Where appended Markdown goes: the index just before the body's final
/// newline, and whether the current last paragraph has content (in which case
/// a paragraph break is inserted first).
fn append_point(document: &Value) -> Result<(u64, bool), GwsError> {
    let content = document
        .pointer("/body/content")
        .and_then(Value::as_array)
        .ok_or_else(|| http::other_err(anyhow::anyhow!("Document has no body content")))?;
    let last = content
        .last()
        .ok_or_else(|| http::other_err(anyhow::anyhow!("Document body is empty")))?;
    let end = last
        .get("endIndex")
        .and_then(Value::as_u64)
        .ok_or_else(|| http::other_err(anyhow::anyhow!("Document body has no endIndex")))?;
    let last_para_has_text = last
        .pointer("/paragraph/elements")
        .and_then(Value::as_array)
        .map(|els| {
            els.iter().any(|e| {
                e.pointer("/textRun/content")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t != "\n")
                    || e.get("textRun").is_none()
            })
        })
        // A table or other non-paragraph element at the end: start fresh.
        .unwrap_or(true);
    Ok((end.saturating_sub(1).max(1), last_para_has_text))
}

async fn write_markdown(api: &Api, document_id: &str, md: &str) -> Result<Value, GwsError> {
    let rendered = markdown::render(md)?;
    let (index, needs_break) = if api.is_dry_run() {
        api.plan_note(json!({
            "note": "the real run first reads the document to find its end index; this plan assumes an empty document (index 1)"
        }))?;
        (1, false)
    } else {
        append_point(&get_document(api, document_id).await?)?
    };
    let requests = rendered.to_requests(index, needs_break);
    api.send(batch_update_request(api, document_id, requests))
        .await
}

async fn write(api: &Api, document_id: &str, content: &Content) -> Result<Value, GwsError> {
    if content.markdown {
        return write_markdown(api, document_id, &content.text).await;
    }
    api.send(batch_update_request(
        api,
        document_id,
        vec![json!({
            "insertText": {
                "text": content.text,
                "endOfSegmentLocation": { "segmentId": "" }
            }
        })],
    ))
    .await
}

async fn create(api: &Api, title: &str, content: Option<&Content>) -> Result<Value, GwsError> {
    let created = api
        .send(ApiRequest::post(api.url("v1/documents")).json(json!({ "title": title })))
        .await?;
    let id = if api.is_dry_run() {
        "<new documentId>".to_string()
    } else {
        created
            .get("documentId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                http::other_err(anyhow::anyhow!("documents.create returned no documentId"))
            })?
            .to_string()
    };
    let url = format!("https://docs.google.com/document/d/{id}/edit");
    if let Some(content) = content {
        let written = if content.markdown {
            match markdown::render(&content.text) {
                Ok(rendered) => api
                    .send(batch_update_request(
                        api,
                        &id,
                        rendered.to_requests(1, false),
                    ))
                    .await
                    .map(drop),
                Err(e) => Err(e),
            }
        } else {
            write(api, &id, content).await.map(drop)
        };
        // The document already exists: name it, so the content can be added
        // with +write instead of a rerun that creates a second document.
        written.map_err(|e| content_write_failed(e, &id, &url))?;
    }
    Ok(json!({
        "documentId": id,
        "title": title,
        "url": url,
    }))
}

/// Prefix an error from writing a new document's content with the document
/// that `documents.create` already made. The error keeps its class (and so
/// its exit code).
fn content_write_failed(err: GwsError, id: &str, url: &str) -> GwsError {
    let context = format!(
        "document {id} was created ({url}) but writing its content failed; \
         add the content with `gwsr docs +write --document-id {id}` rather than \
         rerunning +create"
    );
    match err {
        GwsError::Validation(m) => GwsError::Validation(format!("{context}: {m}")),
        GwsError::Auth(m) => GwsError::Auth(format!("{context}: {m}")),
        GwsError::Config(m) => GwsError::Config(format!("{context}: {m}")),
        GwsError::CredentialStore(m) => GwsError::CredentialStore(format!("{context}: {m}")),
        GwsError::Discovery(m) => GwsError::Discovery(format!("{context}: {m}")),
        GwsError::ConfirmationRequired(m) => {
            GwsError::ConfirmationRequired(format!("{context}: {m}"))
        }
        GwsError::SanitizationBlocked(m) => {
            GwsError::SanitizationBlocked(format!("{context}: {m}"))
        }
        other => crate::transport::errors::with_context(other, &context),
    }
}

fn check_find(find: &str) -> Result<(), GwsError> {
    if find.is_empty() {
        return Err(GwsError::Validation("--find must not be empty".into()));
    }
    Ok(())
}

async fn replace(
    api: &Api,
    document_id: &str,
    find: &str,
    replace_with: &str,
    match_case: bool,
) -> Result<Value, GwsError> {
    check_find(find)?;
    let resp = api
        .send(batch_update_request(
            api,
            document_id,
            vec![json!({
                "replaceAllText": {
                    "containsText": { "text": find, "matchCase": match_case },
                    "replaceText": replace_with
                }
            })],
        ))
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    let changed = resp
        .pointer("/replies/0/replaceAllText/occurrencesChanged")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if changed == 0 {
        tracing::warn!("no occurrences of the --find text were found");
    }
    Ok(json!({ "documentId": document_id, "occurrencesChanged": changed }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn parse(args: &[&str]) -> ArgMatches {
        DocsHelper
            .inject_commands(
                Command::new("gwsr"),
                &crate::discovery::RestDescription::default(),
            )
            .try_get_matches_from(args)
            .unwrap()
    }

    #[test]
    fn write_requires_content_and_document() {
        let cmd = DocsHelper.inject_commands(
            Command::new("gwsr"),
            &crate::discovery::RestDescription::default(),
        );
        assert!(
            cmd.clone()
                .try_get_matches_from(["gwsr", "+write", "--document-id", "D"])
                .is_err()
        );
        assert!(
            cmd.try_get_matches_from(["gwsr", "+write", "--text", "x"])
                .is_err()
        );
    }

    #[test]
    fn content_and_find_may_start_with_a_hyphen() {
        // A Markdown list or rule starts with '-'; clap must not read it
        // as a flag.
        let m = parse(&[
            "gwsr",
            "+write",
            "--document-id",
            "D",
            "--markdown",
            "--text",
            "- item one\n- item two",
        ]);
        let c = Content::parse(m.subcommand_matches("+write").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(c.text, "- item one\n- item two");
        let m = parse(&["gwsr", "+create", "--title", "T", "--text", "---"]);
        let c = Content::parse(m.subcommand_matches("+create").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(c.text, "---");
        let m = parse(&[
            "gwsr",
            "+replace",
            "--document-id",
            "D",
            "--find",
            "-5 C",
            "--replace-with",
            "-10 C",
        ]);
        let sub = m.subcommand_matches("+replace").unwrap();
        assert_eq!(required(sub, "find").unwrap(), "-5 C");
    }

    #[test]
    fn content_parse_markdown_flag() {
        let m = parse(&[
            "gwsr",
            "+write",
            "--document-id",
            "D",
            "--text",
            "# H",
            "--markdown",
        ]);
        let c = Content::parse(m.subcommand_matches("+write").unwrap())
            .unwrap()
            .unwrap();
        assert!(c.markdown);
        assert_eq!(c.text, "# H");
    }

    #[test]
    fn text_file_drops_a_utf8_bom() {
        // Windows editors save UTF-8 with a BOM. Kept, it is an invisible
        // character in the document and, with --markdown, it stops the first
        // line from being read as a heading.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "\u{feff}# Title\r\n\r\nBody\r\n").unwrap();
        let file = file.to_str().unwrap();
        let m = parse(&[
            "gwsr",
            "+write",
            "--document-id",
            "D",
            "--text-file",
            file,
            "--markdown",
        ]);
        let c = Content::parse(m.subcommand_matches("+write").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(c.text, "# Title\r\n\r\nBody\r\n");
        let reqs = markdown::render(&c.text).unwrap().to_requests(1, false);
        assert_eq!(reqs[0]["insertText"]["text"], "Title\nBody");
        assert!(
            reqs.iter().any(
                |r| r.pointer("/updateParagraphStyle/paragraphStyle/namedStyleType")
                    == Some(&json!("HEADING_1"))
            ),
            "{reqs:?}"
        );
    }

    #[test]
    fn create_markdown_without_content_is_error() {
        let m = parse(&["gwsr", "+create", "--title", "T", "--markdown"]);
        assert!(Content::parse(m.subcommand_matches("+create").unwrap()).is_err());
    }

    #[tokio::test]
    async fn plain_write_appends_at_end_of_segment() {
        let api = dry_api("");
        write(
            &api,
            "DOC",
            &Content {
                text: "hello".into(),
                markdown: false,
            },
        )
        .await
        .unwrap();
        let plan = api.planned();
        assert!(
            plan[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/v1/documents/DOC:batchUpdate")
        );
        assert_eq!(
            plan[0]["body"]["requests"][0]["insertText"]["text"],
            "hello"
        );
    }

    #[test]
    fn append_point_detects_non_empty_last_paragraph() {
        let doc = json!({"body": {"content": [
            {"endIndex": 1, "sectionBreak": {}},
            {"startIndex": 1, "endIndex": 7, "paragraph": {"elements": [
                {"textRun": {"content": "Hello\n"}}
            ]}}
        ]}});
        assert_eq!(append_point(&doc).unwrap(), (6, true));
        let empty = json!({"body": {"content": [
            {"endIndex": 1, "sectionBreak": {}},
            {"startIndex": 1, "endIndex": 2, "paragraph": {"elements": [
                {"textRun": {"content": "\n"}}
            ]}}
        ]}});
        assert_eq!(append_point(&empty).unwrap(), (1, false));
    }

    #[tokio::test]
    async fn markdown_write_reads_end_index_then_updates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/documents/DOC"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"body": {"content": [
                    {"endIndex": 1, "sectionBreak": {}},
                    {"startIndex": 1, "endIndex": 7, "paragraph": {"elements": [
                        {"textRun": {"content": "Hello\n"}}
                    ]}}
                ]}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/documents/DOC:batchUpdate"))
            .and(body_partial_json(json!({"requests": [
                {"insertText": {"location": {"index": 6}, "text": "\nTitle"}}
            ]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"replies": []})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        write(
            &api,
            "DOC",
            &Content {
                text: "# Title".into(),
                markdown: true,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_with_markdown_plans_two_requests() {
        let api = dry_api("");
        let v = create(
            &api,
            "T",
            Some(&Content {
                text: "**b**".into(),
                markdown: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(v["documentId"], "<new documentId>");
        let plan = api.planned();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0]["body"]["title"], "T");
    }

    #[tokio::test]
    async fn create_names_the_new_document_when_writing_content_fails() {
        // documents.create succeeded, so the document exists: the error must
        // say which one, or a rerun of +create makes a second document.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"documentId": "NEWDOC"})))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/documents/NEWDOC:batchUpdate"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {"code": 400, "message": "Invalid requests[0]", "status": "INVALID_ARGUMENT"}
            })))
            .expect(2)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        for markdown in [true, false] {
            let err = create(
                &api,
                "T",
                Some(&Content {
                    text: "hello".into(),
                    markdown,
                }),
            )
            .await
            .unwrap_err();
            let text = err.to_string();
            assert!(text.contains("NEWDOC"), "{text}");
            assert!(text.contains("+write --document-id NEWDOC"), "{text}");
            assert!(text.contains("Invalid requests[0]"), "{text}");
            // Still the API's error class and exit code.
            assert!(matches!(err, GwsError::Api { code: 400, .. }), "{err:?}");
        }
    }

    #[tokio::test]
    async fn replace_reports_occurrences() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/documents/DOC:batchUpdate"))
            .and(body_partial_json(json!({"requests": [{"replaceAllText": {
                "containsText": {"text": "{{x}}", "matchCase": true}, "replaceText": "y"
            }}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "replies": [{"replaceAllText": {"occurrencesChanged": 3}}]
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = replace(&api, "DOC", "{{x}}", "y", true).await.unwrap();
        assert_eq!(v["occurrencesChanged"], 3);
        assert!(replace(&api, "DOC", "", "y", true).await.is_err());
    }
}
