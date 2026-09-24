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

//! Forms helpers: `+responses` (JSON rows, or CSV via `--output`).

use super::Helper;
use super::http::{self, Api, ApiRequest, OutputTarget};
use crate::args::{flag, optional, required};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Map, Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_BODY_READONLY: &str = "https://www.googleapis.com/auth/forms.body.readonly";
const SCOPE_RESPONSES_READONLY: &str = "https://www.googleapis.com/auth/forms.responses.readonly";

const FIXED_COLUMNS: [&str; 4] = [
    "responseId",
    "createTime",
    "lastSubmittedTime",
    "respondentEmail",
];

pub struct FormsHelper;

impl Helper for FormsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+responses")
                .about("[Helper] Export all responses of a form as rows (JSON or CSV)")
                .arg(
                    Arg::new("form-id")
                        .long("form-id")
                        .help("Form ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Write CSV to this path (under the current directory), or '-' for CSV on stdout")
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
  gwsr forms +responses --form-id FORM_ID
  gwsr forms +responses --form-id FORM_ID --output responses.csv

TIPS:
  Read-only. One row per response; one column per question (grid rows get
  their own column). Multiple answers in a cell are joined with '; ';
  file uploads are listed by file name.",
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
            let Some(("+responses", m)) = matches.subcommand() else {
                return Ok(false);
            };
            let target = optional(m, "output")?
                .map(|o| OutputTarget::parse(o, flag(m, "overwrite")?))
                .transpose()?;
            let api = Api::new(
                doc,
                &[SCOPE_BODY_READONLY, SCOPE_RESPONSES_READONLY],
                crate::args::dry_run(m)?,
                sanitize,
            )
            .await?;
            let table = responses(&api, required(m, "form-id")?).await?;
            let Some(table) = table else {
                api.emit(m, &Value::Null).await?;
                return Ok(true);
            };
            match target {
                None => api.emit(m, &table.to_json()).await?,
                Some(OutputTarget::Stdout) => {
                    let csv = table.to_csv()?;
                    api.emit_text(m, csv.trim_end_matches('\n')).await?;
                }
                Some(OutputTarget::File { path, overwrite }) => {
                    let csv = table.to_csv()?;
                    api.screen_text(&csv).await?;
                    crate::output_file::write_atomic(&path, csv.as_bytes(), overwrite).await?;
                    api.emit(
                        m,
                        &json!({"output": path.display().to_string(), "rows": table.rows.len()}),
                    )
                    .await?;
                }
            }
            Ok(true)
        })
    }
}

/// A question column: API question ID and header text.
#[derive(Debug, PartialEq)]
struct Column {
    id: String,
    title: String,
}

#[derive(Debug)]
struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
}

/// Question columns in form order.
fn columns(form: &Value) -> Vec<Column> {
    let mut cols = Vec::new();
    for item in form
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(id) = item
            .pointer("/questionItem/question/questionId")
            .and_then(Value::as_str)
        {
            cols.push(Column {
                id: id.into(),
                title,
            });
        } else if let Some(qs) = item
            .pointer("/questionGroupItem/questions")
            .and_then(Value::as_array)
        {
            for q in qs {
                if let Some(id) = q.get("questionId").and_then(Value::as_str) {
                    let row = q
                        .pointer("/rowQuestion/title")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    cols.push(Column {
                        id: id.into(),
                        title: format!("{title} [{row}]"),
                    });
                }
            }
        }
    }
    cols
}

fn answer_text(answer: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for a in answer
        .pointer("/textAnswers/answers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(v) = a.get("value").and_then(Value::as_str) {
            parts.push(v.to_string());
        }
    }
    for a in answer
        .pointer("/fileUploadAnswers/answers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(v) = a
            .get("fileName")
            .or_else(|| a.get("fileId"))
            .and_then(Value::as_str)
        {
            parts.push(v.to_string());
        }
    }
    parts.join("; ")
}

fn build_table(form: &Value, responses: &[Value]) -> Table {
    let columns = columns(form);
    let rows = responses
        .iter()
        .map(|r| {
            let mut row: Vec<String> = FIXED_COLUMNS
                .iter()
                .map(|k| r.get(*k).and_then(Value::as_str).unwrap_or("").to_string())
                .collect();
            for c in &columns {
                row.push(
                    r.pointer(&format!("/answers/{}", c.id))
                        .map(answer_text)
                        .unwrap_or_default(),
                );
            }
            row
        })
        .collect();
    Table { columns, rows }
}

impl Table {
    fn headers(&self) -> Vec<String> {
        FIXED_COLUMNS
            .iter()
            .map(|s| s.to_string())
            .chain(self.columns.iter().map(|c| c.title.clone()))
            .collect()
    }

    fn to_json(&self) -> Value {
        let headers = self.headers();
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|r| {
                let mut obj = Map::new();
                for (h, v) in headers.iter().zip(r) {
                    obj.insert(h.clone(), json!(v));
                }
                Value::Object(obj)
            })
            .collect();
        json!({
            "questions": self.columns.iter().map(|c| json!({"questionId": c.id, "title": c.title})).collect::<Vec<_>>(),
            "responses": rows,
            "count": self.rows.len(),
        })
    }

    fn to_csv(&self) -> Result<String, GwsError> {
        let mut w = csv::Writer::from_writer(Vec::new());
        let enc = |e: csv::Error| http::other_err(anyhow::anyhow!("Failed to encode CSV: {e}"));
        w.write_record(self.headers()).map_err(enc)?;
        for r in &self.rows {
            w.write_record(r).map_err(enc)?;
        }
        let bytes = w
            .into_inner()
            .map_err(|e| http::other_err(anyhow::anyhow!("Failed to encode CSV: {e}")))?;
        String::from_utf8(bytes).map_err(http::other_err)
    }
}

async fn responses(api: &Api, form_id: &str) -> Result<Option<Table>, GwsError> {
    let base = format!("v1/forms/{}", encode_path_segment(form_id));
    let form = api.send(ApiRequest::get(api.url(&base))).await?;
    let page = api
        .paginate(
            ApiRequest::get(api.url(&format!("{base}/responses"))).query("pageSize", "5000"),
            "responses",
            None,
        )
        .await?;
    if api.is_dry_run() {
        return Ok(None);
    }
    Ok(Some(build_table(&form, &page.items)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::api;
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn form() -> Value {
        json!({"items": [
            {"title": "Name", "questionItem": {"question": {"questionId": "q1"}}},
            {"title": "Section header"},
            {"title": "Rate", "questionGroupItem": {"questions": [
                {"questionId": "g1", "rowQuestion": {"title": "Speed"}},
                {"questionId": "g2", "rowQuestion": {"title": "Cost"}}
            ]}},
            {"title": "Colors", "questionItem": {"question": {"questionId": "q2"}}}
        ]})
    }

    #[test]
    fn columns_include_grid_rows_and_skip_non_questions() {
        let titles: Vec<String> = columns(&form()).into_iter().map(|c| c.title).collect();
        assert_eq!(
            titles,
            vec!["Name", "Rate [Speed]", "Rate [Cost]", "Colors"]
        );
    }

    #[test]
    fn table_to_csv_and_json() {
        let responses = vec![json!({
            "responseId": "r1", "createTime": "t", "lastSubmittedTime": "t2",
            "answers": {
                "q1": {"textAnswers": {"answers": [{"value": "Ann, Jr."}]}},
                "q2": {"textAnswers": {"answers": [{"value": "red"}, {"value": "blue"}]}},
                "g1": {"textAnswers": {"answers": [{"value": "5"}]}}
            }
        })];
        let t = build_table(&form(), &responses);
        let csv = t.to_csv().unwrap();
        assert_eq!(
            csv,
            "responseId,createTime,lastSubmittedTime,respondentEmail,Name,Rate [Speed],Rate [Cost],Colors\nr1,t,t2,,\"Ann, Jr.\",5,,red; blue\n"
        );
        let j = t.to_json();
        assert_eq!(j["responses"][0]["Colors"], "red; blue");
        assert_eq!(j["count"], 1);
    }

    #[tokio::test]
    async fn responses_fetches_form_and_all_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/forms/F1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(form()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/forms/F1/responses"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"responses": [{"responseId": "r1"}, {"responseId": "r2"}]}),
                ),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let t = responses(&api, "F1").await.unwrap().unwrap();
        assert_eq!(t.rows.len(), 2);
    }
}
