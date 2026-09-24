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

//! Sheets helpers: `+append`, `+read`, `+write`, `+clear`, `+create`, with CSV
//! import (`--csv-file`) and export (`+read --output`).

use super::Helper;
use super::http::{self, Api, ApiRequest, OutputTarget};
use crate::args::{flag, many, optional, required};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_SHEETS: &str = "https://www.googleapis.com/auth/spreadsheets";
const SCOPE_SHEETS_READONLY: &str = "https://www.googleapis.com/auth/spreadsheets.readonly";

pub struct SheetsHelper;

fn spreadsheet_id_arg() -> Arg {
    Arg::new("spreadsheet-id")
        .long("spreadsheet-id")
        .help("Spreadsheet ID")
        .required(true)
        .value_name("ID")
}

fn range_arg(required: bool, help: &'static str) -> Arg {
    Arg::new("range")
        .long("range")
        .help(help)
        .required(required)
        .value_name("RANGE")
}

/// Value input arguments shared by `+append` and `+write`.
fn value_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("values")
            .long("values")
            .help("One row as CSV (quote cells containing commas: 'a,\"b,c\",d')")
            .value_name("CSV_ROW"),
    )
    .arg(
        Arg::new("json-values")
            .long("json-values")
            .help("JSON array of rows, e.g. '[[\"a\",1],[\"b\",2]]' (a flat array is one row)")
            .value_name("JSON"),
    )
    .arg(
        Arg::new("csv-file")
            .long("csv-file")
            .help("Import rows from a CSV file under the current directory, or '-' for stdin")
            .value_name("PATH"),
    )
    .group(
        ArgGroup::new("input")
            .args(["values", "json-values", "csv-file"])
            .required(true),
    )
    .arg(
        Arg::new("raw")
            .long("raw")
            .help("Store input as-is (RAW) instead of parsing it like typed input (USER_ENTERED: formulas, numbers, dates)")
            .action(ArgAction::SetTrue),
    )
}

impl Helper for SheetsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(value_args(
            Command::new("+append")
                .about("[Helper] Append rows after the last row of a table")
                .arg(spreadsheet_id_arg())
                .arg(range_arg(
                    false,
                    "Table to append to in A1 notation, e.g. 'Sheet2!A1' (default: A1 of the first sheet)",
                ))
                .after_help(
                    r#"EXAMPLES:
  gwsr sheets +append --spreadsheet-id ID --values 'Alice,100,true'
  gwsr sheets +append --spreadsheet-id ID --json-values '[["a","b"],["c","d"]]'
  gwsr sheets +append --spreadsheet-id ID --range 'Sheet2!A1' --csv-file ./rows.csv

TIPS:
  Rows are inserted (INSERT_ROWS), never overwriting existing data.
  Input is parsed like typed input unless --raw is given."#,
                ),
        ))
        .subcommand(
            Command::new("+read")
                .about("[Helper] Read values from a range, optionally exporting CSV")
                .arg(spreadsheet_id_arg())
                .arg(range_arg(true, "Range to read, e.g. 'Sheet1!A1:D10' or 'Sheet1'"))
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Write the values as CSV to this path (under the current directory), or '-' for stdout")
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
  gwsr sheets +read --spreadsheet-id ID --range 'Sheet1!A1:D10'
  gwsr sheets +read --spreadsheet-id ID --range Sheet1 --output sheet1.csv
  gwsr sheets +read --spreadsheet-id ID --range Sheet1 --output -

TIPS:
  Read-only. Values are the formatted strings shown in the UI.",
                ),
        )
        .subcommand(value_args(
            Command::new("+write")
                .about("[Helper] Overwrite values in a range (values.update)")
                .arg(spreadsheet_id_arg())
                .arg(range_arg(true, "Top-left cell or range to write, e.g. 'Sheet1!B2'"))
                .after_help(
                    r#"EXAMPLES:
  gwsr sheets +write --spreadsheet-id ID --range 'Sheet1!B2' --values 'x,y,z'
  gwsr sheets +write --spreadsheet-id ID --range 'Sheet1!A1' --json-values '[["Name","Score"],["Ann",9]]'
  gwsr sheets +write --spreadsheet-id ID --range 'Import!A1' --csv-file data.csv

TIPS:
  Existing cells in the written area are overwritten.
  Use +clear first to remove stale data outside the new values."#,
                ),
        ))
        .subcommand(with_yes(
            Command::new("+clear")
                .about("[Helper] Clear all values in a range (formatting is kept)")
                .arg(spreadsheet_id_arg())
                .arg(range_arg(true, "Range to clear, e.g. 'Sheet1!A2:Z'"))
                .after_help(
                    "\
EXAMPLES:
  gwsr sheets +clear --spreadsheet-id ID --range 'Sheet1!A2:Z' --yes

TIPS:
  Destructive: requires --yes (or a confirmation prompt on a terminal).",
                ),
        ))
        .subcommand(
            Command::new("+create")
                .about("[Helper] Create a new spreadsheet")
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Spreadsheet title")
                        .required(true)
                        .value_name("TITLE"),
                )
                .arg(
                    Arg::new("sheet")
                        .long("sheet")
                        .help("Sheet (tab) name; repeat for several (default: one 'Sheet1')")
                        .action(ArgAction::Append)
                        .value_name("NAME"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr sheets +create --title 'Budget 2026'
  gwsr sheets +create --title 'Tracker' --sheet Tasks --sheet Archive

TIPS:
  Prints spreadsheetId and url.",
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
            let id = || required(m, "spreadsheet-id");
            let (api, value) = match name {
                "+append" => {
                    let rows = parse_values(m)?;
                    let api = Api::new(doc, &[SCOPE_SHEETS], dry, sanitize).await?;
                    let range = optional(m, "range")?.unwrap_or("A1");
                    let v = append(&api, id()?, range, rows, flag(m, "raw")?).await?;
                    (api, v)
                }
                "+write" => {
                    let rows = parse_values(m)?;
                    let api = Api::new(doc, &[SCOPE_SHEETS], dry, sanitize).await?;
                    let v =
                        write(&api, id()?, required(m, "range")?, rows, flag(m, "raw")?).await?;
                    (api, v)
                }
                "+read" => {
                    let target = optional(m, "output")?
                        .map(|o| OutputTarget::parse(o, flag(m, "overwrite")?))
                        .transpose()?;
                    let api = Api::new(doc, &[SCOPE_SHEETS_READONLY], dry, sanitize).await?;
                    let v = read(&api, id()?, required(m, "range")?).await?;
                    if let (Some(target), false) = (&target, api.is_dry_run()) {
                        let csv = values_to_csv(v.get("values"))?;
                        match target {
                            OutputTarget::Stdout => {
                                api.emit_text(m, csv.trim_end_matches('\n')).await?;
                            }
                            OutputTarget::File { path, overwrite } => {
                                api.screen_text(&csv).await?;
                                crate::output_file::write_atomic(path, csv.as_bytes(), *overwrite)
                                    .await?;
                                let rows = v
                                    .get("values")
                                    .and_then(Value::as_array)
                                    .map_or(0, Vec::len);
                                api.emit(
                                    m,
                                    &json!({"output": path.display().to_string(), "rows": rows}),
                                )
                                .await?;
                            }
                        }
                        return Ok(true);
                    }
                    (api, v)
                }
                "+clear" => {
                    let range = required(m, "range")?;
                    confirm::confirm(
                        m,
                        Impact::Destructive,
                        &format!("clear all values in {range} of spreadsheet {}", id()?),
                    )?;
                    let api = Api::new(doc, &[SCOPE_SHEETS], dry, sanitize).await?;
                    let v = clear(&api, id()?, range).await?;
                    (api, v)
                }
                "+create" => {
                    let api = Api::new(doc, &[SCOPE_SHEETS], dry, sanitize).await?;
                    let v = create(&api, required(m, "title")?, &many(m, "sheet")?).await?;
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

fn scalar(v: Value, row: usize, col: usize) -> Result<Value, GwsError> {
    match v {
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => Ok(v),
        _ => Err(GwsError::Validation(format!(
            "--json-values: cell at row {} column {} must be a string, number, boolean or null",
            row + 1,
            col + 1
        ))),
    }
}

/// Parse `--json-values`: an array of rows, or a flat array meaning one row.
fn parse_json_rows(s: &str) -> Result<Vec<Vec<Value>>, GwsError> {
    let parsed: Value = serde_json::from_str(s)
        .map_err(|e| GwsError::Validation(format!("--json-values is not valid JSON: {e}")))?;
    let Value::Array(items) = parsed else {
        return Err(GwsError::Validation(
            "--json-values must be a JSON array of rows or a flat array".into(),
        ));
    };
    if items.is_empty() {
        return Err(GwsError::Validation("--json-values is empty".into()));
    }
    let rows = if items.iter().all(Value::is_array) {
        items
            .into_iter()
            .map(|r| match r {
                Value::Array(cells) => cells,
                _ => Vec::new(),
            })
            .collect()
    } else if items.iter().any(Value::is_array) {
        return Err(GwsError::Validation(
            "--json-values mixes rows (arrays) and cells; use an array of arrays".into(),
        ));
    } else {
        vec![items]
    };
    rows.into_iter()
        .enumerate()
        .map(|(r, cells)| {
            cells
                .into_iter()
                .enumerate()
                .map(|(c, v)| scalar(v, r, c))
                .collect()
        })
        .collect()
}

/// Parse CSV text into rows of string cells.
fn parse_csv(text: &str, what: &str) -> Result<Vec<Vec<Value>>, GwsError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| {
            GwsError::Validation(format!("{what}: invalid CSV at record {}: {e}", i + 1))
        })?;
        rows.push(
            record
                .iter()
                .map(|c| Value::String(c.to_string()))
                .collect(),
        );
    }
    if rows.is_empty() {
        return Err(GwsError::Validation(format!("{what} contains no rows")));
    }
    Ok(rows)
}

fn parse_values(m: &ArgMatches) -> Result<Vec<Vec<Value>>, GwsError> {
    if let Some(row) = optional(m, "values")? {
        let rows = parse_csv(row, "--values")?;
        if rows.len() != 1 {
            return Err(GwsError::Validation(
                "--values takes a single CSV row; use --json-values or --csv-file for several rows"
                    .into(),
            ));
        }
        return Ok(rows);
    }
    if let Some(json) = optional(m, "json-values")? {
        return parse_json_rows(json);
    }
    if let Some(path) = optional(m, "csv-file")? {
        return parse_csv(&http::read_text_input(path, "--csv-file")?, "--csv-file");
    }
    Err(GwsError::Validation(
        "one of --values, --json-values or --csv-file is required".into(),
    ))
}

fn values_url(api: &Api, spreadsheet_id: &str, range: &str, suffix: &str) -> String {
    api.url(&format!(
        "v4/spreadsheets/{}/values/{}{suffix}",
        encode_path_segment(spreadsheet_id),
        encode_path_segment(range)
    ))
}

fn input_option(raw: bool) -> &'static str {
    if raw { "RAW" } else { "USER_ENTERED" }
}

async fn append(
    api: &Api,
    spreadsheet_id: &str,
    range: &str,
    rows: Vec<Vec<Value>>,
    raw: bool,
) -> Result<Value, GwsError> {
    api.send(
        ApiRequest::post(values_url(api, spreadsheet_id, range, ":append"))
            .query("valueInputOption", input_option(raw))
            .query("insertDataOption", "INSERT_ROWS")
            .json(json!({ "range": range, "majorDimension": "ROWS", "values": rows })),
    )
    .await
}

async fn write(
    api: &Api,
    spreadsheet_id: &str,
    range: &str,
    rows: Vec<Vec<Value>>,
    raw: bool,
) -> Result<Value, GwsError> {
    api.send(
        ApiRequest::put(values_url(api, spreadsheet_id, range, ""))
            .query("valueInputOption", input_option(raw))
            .json(json!({ "range": range, "majorDimension": "ROWS", "values": rows })),
    )
    .await
}

async fn read(api: &Api, spreadsheet_id: &str, range: &str) -> Result<Value, GwsError> {
    api.send(ApiRequest::get(values_url(api, spreadsheet_id, range, "")))
        .await
}

async fn clear(api: &Api, spreadsheet_id: &str, range: &str) -> Result<Value, GwsError> {
    api.send(ApiRequest::post(values_url(api, spreadsheet_id, range, ":clear")).json(json!({})))
        .await
}

async fn create(api: &Api, title: &str, sheets: &[String]) -> Result<Value, GwsError> {
    let mut body = json!({ "properties": { "title": title } });
    if !sheets.is_empty() {
        body["sheets"] = Value::Array(
            sheets
                .iter()
                .map(|s| json!({ "properties": { "title": s } }))
                .collect(),
        );
    }
    let resp = api
        .send(ApiRequest::post(api.url("v4/spreadsheets")).json(body))
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    Ok(json!({
        "spreadsheetId": resp.get("spreadsheetId"),
        "title": title,
        "url": resp.get("spreadsheetUrl"),
        "sheets": resp.get("sheets").and_then(Value::as_array).map(|a| {
            a.iter().filter_map(|s| s.pointer("/properties/title").cloned()).collect::<Vec<_>>()
        }),
    }))
}

/// Serialize a `values` array as CSV.
fn values_to_csv(values: Option<&Value>) -> Result<String, GwsError> {
    let mut writer = csv::WriterBuilder::new()
        .flexible(true)
        .from_writer(Vec::new());
    if let Some(rows) = values.and_then(Value::as_array) {
        for row in rows {
            let cells: Vec<String> = row
                .as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| match c {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            writer
                .write_record(&cells)
                .map_err(|e| http::other_err(anyhow::anyhow!("Failed to encode CSV: {e}")))?;
        }
    }
    let bytes = writer
        .into_inner()
        .map_err(|e| http::other_err(anyhow::anyhow!("Failed to encode CSV: {e}")))?;
    String::from_utf8(bytes).map_err(http::other_err)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn parse(args: &[&str]) -> ArgMatches {
        SheetsHelper
            .inject_commands(
                Command::new("gwsr"),
                &crate::discovery::RestDescription::default(),
            )
            .try_get_matches_from(args)
            .unwrap()
    }

    fn values_of(args: &[&str]) -> Result<Vec<Vec<Value>>, GwsError> {
        let m = parse(args);
        let (_, sub) = m.subcommand().unwrap();
        parse_values(sub)
    }

    #[test]
    fn values_csv_row_handles_quotes() {
        let rows = values_of(&[
            "gwsr",
            "+append",
            "--spreadsheet-id",
            "S",
            "--values",
            "a,\"b,c\",d",
        ])
        .unwrap();
        assert_eq!(rows, vec![vec![json!("a"), json!("b,c"), json!("d")]]);
    }

    #[test]
    fn json_values_accepts_scalars_and_rejects_bad_input() {
        assert_eq!(
            parse_json_rows(r#"[["a", 1, true, null]]"#).unwrap(),
            vec![vec![json!("a"), json!(1), json!(true), Value::Null]]
        );
        assert_eq!(
            parse_json_rows(r#"["x","y"]"#).unwrap(),
            vec![vec![json!("x"), json!("y")]]
        );
        assert!(parse_json_rows("not json").is_err());
        assert!(parse_json_rows("[]").is_err());
        assert!(parse_json_rows(r#"[["a"], "b"]"#).is_err());
        assert!(parse_json_rows(r#"[[{"a":1}]]"#).is_err());
        assert!(parse_json_rows(r#"{"a":1}"#).is_err());
    }

    #[test]
    fn exactly_one_input_is_required() {
        let cmd = SheetsHelper.inject_commands(
            Command::new("gwsr"),
            &crate::discovery::RestDescription::default(),
        );
        assert!(
            cmd.clone()
                .try_get_matches_from(["gwsr", "+append", "--spreadsheet-id", "S"])
                .is_err()
        );
        assert!(
            cmd.try_get_matches_from([
                "gwsr",
                "+append",
                "--spreadsheet-id",
                "S",
                "--values",
                "a",
                "--json-values",
                "[1]"
            ])
            .is_err()
        );
    }

    #[test]
    fn csv_parsing_multi_row_and_empty() {
        assert_eq!(parse_csv("a,b\nc,d\n", "x").unwrap().len(), 2);
        assert!(parse_csv("", "x").is_err());
    }

    #[test]
    fn csv_export_quotes_cells() {
        let v = json!([["a", "b,c"], ["1", null, 2]]);
        assert_eq!(values_to_csv(Some(&v)).unwrap(), "a,\"b,c\"\n1,,2\n");
    }

    #[tokio::test]
    async fn append_request_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v4/spreadsheets/S1/values/Sheet2%21A1:append"))
            .and(query_param("valueInputOption", "RAW"))
            .and(query_param("insertDataOption", "INSERT_ROWS"))
            .and(body_partial_json(json!({"values": [["a", 1]]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"updates": {}})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        append(
            &api,
            "S1",
            "Sheet2!A1",
            vec![vec![json!("a"), json!(1)]],
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn write_uses_put_update() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/v4/spreadsheets/S1/values/A1"))
            .and(query_param("valueInputOption", "USER_ENTERED"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"updatedCells": 1})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = write(&api, "S1", "A1", vec![vec![json!("x")]], false)
            .await
            .unwrap();
        assert_eq!(v["updatedCells"], 1);
    }

    #[tokio::test]
    async fn clear_and_create_plans() {
        let api = dry_api("");
        clear(&api, "S1", "Sheet1!A2:Z").await.unwrap();
        create(&api, "T", &["Tasks".into(), "Archive".into()])
            .await
            .unwrap();
        let plan = api.planned();
        assert!(plan[0]["url"].as_str().unwrap().ends_with(":clear"));
        assert_eq!(
            plan[1]["body"]["sheets"][1]["properties"]["title"],
            "Archive"
        );
    }

    #[test]
    fn clear_requires_confirmation_without_tty() {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            return;
        }
        let m = parse(&[
            "gwsr",
            "+clear",
            "--spreadsheet-id",
            "S",
            "--range",
            "A1:B2",
        ]);
        let sub = m.subcommand_matches("+clear").unwrap();
        assert!(confirm::confirm(sub, Impact::Destructive, "clear").is_err());
    }
}
