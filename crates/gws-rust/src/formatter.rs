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

//! Output Formatting
//!
//! Transforms JSON values into the user-selected output format (JSON, table,
//! YAML, CSV), after applying the optional `--jq` filter and `--columns`
//! selection.
//!
//! Process-wide output preferences (`--compact`/`--pretty`, `--jq`,
//! `--columns`) are installed once by `main` through [`install_settings`], so
//! every call site (executor, helpers) formats consistently without threading
//! the options through each signature.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::error::GwsError;
use crate::jq::JqFilter;
use crate::output::sanitize_for_terminal;

/// Values accepted by `--format` (and `format` in the config file).
pub const FORMAT_NAMES: &[&str] = &["json", "table", "yaml", "yml", "csv"];

/// Supported output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// JSON (default). Compact or pretty depending on [`JsonStyle`].
    #[default]
    Json,
    /// Aligned text table.
    Table,
    /// YAML.
    Yaml,
    /// Comma-separated values.
    Csv,
}

impl OutputFormat {
    /// Parse from a string argument.
    ///
    /// Returns `Err(unknown_value)` if the string is not a known format.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "table" => Ok(Self::Table),
            "yaml" | "yml" => Ok(Self::Yaml),
            "csv" => Ok(Self::Csv),
            other => Err(other.to_string()),
        }
    }

    /// Parse a `--format` value, reporting an unknown one as a validation
    /// error (clap normally rejects it first).
    pub fn from_str(s: &str) -> Result<Self, GwsError> {
        Self::parse(s).map_err(|unknown| {
            GwsError::Validation(format!(
                "unknown output format '{unknown}'; use one of: {}",
                FORMAT_NAMES.join(", ")
            ))
        })
    }

    /// Resolve the effective output format from parsed arguments: the
    /// `--format` flag when present, otherwise the configured default.
    pub fn from_matches(matches: &clap::ArgMatches) -> Result<Self, GwsError> {
        match crate::args::optional(matches, "format")? {
            Some(s) => Self::from_str(s),
            None => Ok(settings().default_format),
        }
    }
}

/// How JSON output is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsonStyle {
    /// Single-line JSON (token-efficient; default when stdout is not a TTY).
    Compact,
    /// Indented JSON (default on a terminal).
    #[default]
    Pretty,
}

/// Process-wide output preferences.
#[derive(Debug, Default)]
pub struct OutputSettings {
    /// JSON layout.
    pub json_style: JsonStyle,
    /// Format used when `--format` is absent (from env / config).
    pub default_format: OutputFormat,
    /// `--columns` selection for table and CSV output.
    pub columns: Option<Vec<String>>,
    /// Compiled `--jq` expression.
    pub jq: Option<JqFilter>,
    /// stdout is an interactive terminal: raw (`--jq`) strings are stripped
    /// of control sequences before printing.
    pub terminal: bool,
}

static SETTINGS: OnceLock<OutputSettings> = OnceLock::new();

/// Install the process-wide output settings. May be called once.
pub fn install_settings(settings: OutputSettings) -> Result<(), GwsError> {
    SETTINGS.set(settings).map_err(|_| {
        GwsError::from(anyhow::anyhow!(
            "internal: output settings were installed twice"
        ))
    })
}

fn settings() -> &'static OutputSettings {
    SETTINGS.get_or_init(OutputSettings::default)
}

/// Format a JSON value according to the specified output format and the
/// installed output settings.
///
/// The returned string has no trailing newline; an empty string means
/// "print nothing" (for example a `--jq` filter that produced no output).
pub fn format_value(value: &Value, format: &OutputFormat) -> Result<String, GwsError> {
    render(value, *format, settings(), None)
}

/// Print `value` to stdout in the process-wide default format (`--format` >
/// `GWSR_FORMAT` > config), for commands without their own `--format`.
pub fn emit_default(value: &Value) -> Result<(), GwsError> {
    crate::output::emit(&format_value(value, &settings().default_format)?)
}

/// Format one page of a `--page-all` stream.
///
/// JSON pages are always compact (one JSON document per line, NDJSON). CSV and
/// table output only emit column headers on the first page; YAML pages are
/// prefixed with a `---` document separator.
pub fn format_value_paginated(
    value: &Value,
    format: &OutputFormat,
    is_first_page: bool,
) -> Result<String, GwsError> {
    render(value, *format, settings(), Some(is_first_page))
}

/// Core renderer. `page` is `Some(is_first_page)` in paginated mode.
fn render(
    value: &Value,
    format: OutputFormat,
    settings: &OutputSettings,
    page: Option<bool>,
) -> Result<String, GwsError> {
    let style = if page.is_some() {
        JsonStyle::Compact
    } else {
        settings.json_style
    };

    let Some(jq) = &settings.jq else {
        return render_one(value, format, settings, page, style);
    };

    let outputs = jq.run(value)?;
    if format == OutputFormat::Json {
        // `gh`-style: one result per line, strings printed raw.
        let lines: Vec<String> = outputs
            .iter()
            .map(|v| match v {
                Value::String(s) if settings.terminal => sanitize_for_terminal(s),
                Value::String(s) => s.clone(),
                other => json_string(other, style),
            })
            .collect();
        return Ok(lines.join("\n"));
    }
    match outputs.len() {
        0 => Ok(String::new()),
        1 => render_one(&outputs[0], format, settings, page, style),
        _ => render_one(&Value::Array(outputs), format, settings, page, style),
    }
}

fn render_one(
    value: &Value,
    format: OutputFormat,
    settings: &OutputSettings,
    page: Option<bool>,
    style: JsonStyle,
) -> Result<String, GwsError> {
    let emit_header = page.unwrap_or(true);
    let columns = settings.columns.as_deref();
    let out = match format {
        OutputFormat::Json => json_string(value, style),
        OutputFormat::Table => format_table(value, emit_header, columns)?,
        OutputFormat::Csv => format_csv(value, emit_header, columns)?,
        OutputFormat::Yaml => {
            let yaml = format_yaml(value)?;
            if page.is_some() {
                format!("---\n{yaml}")
            } else {
                yaml
            }
        }
    };
    Ok(out.trim_end_matches('\n').to_string())
}

fn json_string(value: &Value, style: JsonStyle) -> String {
    // Serializing a `serde_json::Value` to a String cannot fail: every key is a
    // string and every number is finite.
    let result = match style {
        JsonStyle::Compact => serde_json::to_string(value),
        JsonStyle::Pretty => serde_json::to_string_pretty(value),
    };
    match result {
        Ok(s) => s,
        Err(e) => format!("{{\"error\":\"failed to serialize output: {e}\"}}"),
    }
}

/// Keys that describe a list response rather than its items. An object is
/// treated as a list envelope only when every non-array key is one of these,
/// so single resources (and `--dry-run` output) keep all their fields.
const LIST_ENVELOPE_KEYS: &[&str] = &[
    "kind",
    "etag",
    "nextPageToken",
    "nextSyncToken",
    "prevPageToken",
    "previousPageToken",
    "incompleteSearch",
    "resultSizeEstimate",
    "totalSize",
    "totalItems",
    "selfLink",
    "nextLink",
    "timeZone",
    "updated",
    "summary",
    "description",
    "accessRole",
    "defaultReminders",
    "range",
    "majorDimension",
    "unreachable",
];

/// Extract the items array from a typical Google API list response such as
/// `{ "files": [...], "nextPageToken": "..." }`.
fn extract_items(value: &Value) -> Option<(&str, &Vec<Value>)> {
    let Value::Object(obj) = value else {
        return None;
    };
    let mut found: Option<(&str, &Vec<Value>)> = None;
    for (key, val) in obj {
        if key.starts_with('_') || LIST_ENVELOPE_KEYS.contains(&key.as_str()) {
            continue;
        }
        match val {
            Value::Array(arr) if found.is_none() => found = Some((key, arr)),
            // A second array or any other non-envelope field: not a list envelope.
            _ => return None,
        }
    }
    found
}

/// Recursively flatten a JSON object into `(dot.notation.key, value)` pairs.
fn flatten_object<'a>(
    obj: &'a Map<String, Value>,
    prefix: &str,
    out: &mut Vec<(String, &'a Value)>,
) {
    for (key, val) in obj {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match val {
            Value::Object(nested) if !nested.is_empty() => flatten_object(nested, &full_key, out),
            _ => out.push((full_key, val)),
        }
    }
}

fn flatten(obj: &Map<String, Value>) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    flatten_object(obj, "", &mut out);
    out
}

/// Rows of an array of objects: `(ordered column names, row maps)`.
///
/// With `selected`, only those columns are kept (in the given order); a
/// selected column that appears in no row is an error so typos are not
/// silently rendered as empty columns.
type Row<'a> = std::collections::HashMap<String, &'a Value>;

fn tabulate<'a>(
    arr: &'a [Value],
    selected: Option<&[String]>,
) -> Result<(Vec<String>, Vec<Row<'a>>), GwsError> {
    let mut columns: Vec<String> = Vec::new();
    let mut rows = Vec::with_capacity(arr.len());
    for item in arr {
        let pairs = match item {
            Value::Object(obj) => flatten(obj),
            other => vec![("value".to_string(), other)],
        };
        for (k, _) in &pairs {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
        rows.push(pairs.into_iter().collect());
    }
    if let Some(selected) = selected {
        let missing: Vec<&str> = selected
            .iter()
            .filter(|c| !columns.contains(c))
            .map(|c| c.as_str())
            .collect();
        if !missing.is_empty() && !arr.is_empty() {
            return Err(GwsError::Validation(format!(
                "--columns: no such column(s): {}. Available: {}",
                missing.join(", "),
                columns.join(", ")
            )));
        }
        columns = selected.to_vec();
    }
    Ok((columns, rows))
}

const MAX_COLUMN_WIDTH: usize = 60;

fn format_table(
    value: &Value,
    emit_header: bool,
    columns: Option<&[String]>,
) -> Result<String, GwsError> {
    if let Some((_key, arr)) = extract_items(value) {
        return format_array_as_table(arr, emit_header, columns);
    }
    match value {
        Value::Array(arr) => format_array_as_table(arr, emit_header, columns),
        Value::Object(obj) => {
            // Single object: key/value table with nested objects flattened.
            let mut flat = flatten(obj);
            if let Some(selected) = columns {
                let missing: Vec<&str> = selected
                    .iter()
                    .filter(|c| !flat.iter().any(|(k, _)| k == *c))
                    .map(|c| c.as_str())
                    .collect();
                if !missing.is_empty() {
                    return Err(GwsError::Validation(format!(
                        "--columns: no such field(s): {}",
                        missing.join(", ")
                    )));
                }
                flat.retain(|(k, _)| selected.contains(k));
            }
            let width = flat
                .iter()
                .map(|(k, _)| k.chars().count())
                .max()
                .unwrap_or(0);
            let mut out = String::new();
            for (key, val) in &flat {
                let line = format!(
                    "{}  {}",
                    pad(&sanitize_for_terminal(key), width),
                    table_cell(val)
                );
                out.push_str(line.trim_end());
                out.push('\n');
            }
            Ok(out)
        }
        other => Ok(table_cell(other)),
    }
}

fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(len)))
}

fn truncate_cell(s: &str, width: usize) -> String {
    if s.chars().count() > width {
        let head: String = s.chars().take(width.saturating_sub(1)).collect();
        format!("{head}…")
    } else {
        s.to_string()
    }
}

fn format_array_as_table(
    arr: &[Value],
    emit_header: bool,
    selected: Option<&[String]>,
) -> Result<String, GwsError> {
    if arr.is_empty() {
        return Ok(if emit_header {
            "(empty)".to_string()
        } else {
            String::new()
        });
    }

    let (columns, rows) = tabulate(arr, selected)?;
    let columns: Vec<String> = columns.iter().map(|c| sanitize_for_terminal(c)).collect();
    let rows: Vec<std::collections::HashMap<String, String>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|(k, v)| (sanitize_for_terminal(k), table_cell(v)))
                .collect()
        })
        .collect();
    // Width: char count, capped.
    let widths: Vec<usize> = columns
        .iter()
        .map(|c| {
            let data = rows
                .iter()
                .map(|r| r.get(c).map_or(0, |v| v.chars().count()))
                .max()
                .unwrap_or(0);
            data.max(c.chars().count()).min(MAX_COLUMN_WIDTH)
        })
        .collect();

    let mut out = String::new();
    if emit_header {
        let header: Vec<String> = columns
            .iter()
            .zip(&widths)
            .map(|(c, w)| pad(&truncate_cell(c, *w), *w))
            .collect();
        out.push_str(header.join("  ").trim_end());
        out.push('\n');
        let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
        out.push_str(&sep.join("  "));
        out.push('\n');
    }
    for row in &rows {
        let cells: Vec<String> = columns
            .iter()
            .zip(&widths)
            .map(|(c, w)| {
                pad(
                    &truncate_cell(row.get(c).map_or("", |v| v.as_str()), *w),
                    *w,
                )
            })
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    Ok(out)
}

fn format_yaml(value: &Value) -> Result<String, GwsError> {
    serde_saphyr::to_string(value)
        .map_err(|e| GwsError::from(anyhow::Error::new(e).context("failed to render YAML output")))
}

fn csv_error(e: csv::Error) -> GwsError {
    GwsError::from(anyhow::Error::new(e).context("failed to render CSV output"))
}

fn format_csv(
    value: &Value,
    emit_header: bool,
    selected: Option<&[String]>,
) -> Result<String, GwsError> {
    let arr: &[Value] = if let Some((_key, arr)) = extract_items(value) {
        arr
    } else if let Value::Array(arr) = value {
        arr
    } else if let Value::Object(obj) = value {
        // Single object: one header row + one data row.
        return format_csv(
            &Value::Array(vec![Value::Object(obj.clone())]),
            emit_header,
            selected,
        );
    } else {
        return Ok(csv_cell(value));
    };

    let mut writer = csv::WriterBuilder::new()
        .flexible(true)
        .from_writer(Vec::new());

    if !arr.iter().any(Value::is_object) {
        // Arrays of rows (e.g. Sheets `values`) or of scalars.
        for item in arr {
            let record: Vec<String> = match item {
                Value::Array(inner) => inner.iter().map(csv_cell).collect(),
                other => vec![csv_cell(other)],
            };
            writer.write_record(&record).map_err(csv_error)?;
        }
    } else {
        let (columns, rows) = tabulate(arr, selected)?;
        if emit_header {
            let header: Vec<String> = columns.iter().map(|c| csv_text(c)).collect();
            writer.write_record(&header).map_err(csv_error)?;
        }
        for row in &rows {
            let record: Vec<String> = columns
                .iter()
                .map(|c| row.get(c).map_or_else(String::new, |v| csv_cell(v)))
                .collect();
            writer.write_record(&record).map_err(csv_error)?;
        }
    }

    let bytes = writer
        .into_inner()
        .map_err(|e| GwsError::from(anyhow::anyhow!("failed to flush CSV output: {e}")))?;
    String::from_utf8(bytes).map_err(|e| {
        GwsError::from(anyhow::Error::new(e).context("CSV output was not valid UTF-8"))
    })
}

/// A table cell: the value's text with terminal control sequences removed.
fn table_cell(value: &Value) -> String {
    sanitize_for_terminal(&value_to_cell(value)).replace(['\n', '\t'], " ")
}

/// Neutralise spreadsheet formula injection (OWASP): a text cell starting with
/// `=`, `+`, `-`, `@`, TAB or CR is prefixed with `'`. Control sequences are
/// stripped.
fn csv_text(s: &str) -> String {
    let clean = sanitize_for_terminal(s);
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{clean}")
    } else {
        clean
    }
}

/// A CSV cell. Only JSON strings can carry formulas; numbers such as `-5`
/// are written unchanged.
fn csv_cell(value: &Value) -> String {
    match value {
        Value::String(s) => csv_text(s),
        other => sanitize_for_terminal(&value_to_cell(other)),
    }
}

fn value_to_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(arr) => arr.iter().map(value_to_cell).collect::<Vec<_>>().join(", "),
        Value::Object(_) => json_string(value, JsonStyle::Compact),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain() -> OutputSettings {
        OutputSettings::default()
    }

    fn fmt(value: &Value, format: OutputFormat) -> String {
        render(value, format, &plain(), None).unwrap()
    }

    fn fmt_with(value: &Value, format: OutputFormat, settings: &OutputSettings) -> String {
        render(value, format, settings, None).unwrap()
    }

    fn page(value: &Value, format: OutputFormat, first: bool) -> String {
        render(value, format, &plain(), Some(first)).unwrap()
    }

    #[test]
    fn output_format_parse() {
        assert_eq!(OutputFormat::parse("json"), Ok(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("TABLE"), Ok(OutputFormat::Table));
        assert_eq!(OutputFormat::parse("yml"), Ok(OutputFormat::Yaml));
        assert_eq!(OutputFormat::parse("csv"), Ok(OutputFormat::Csv));
        assert_eq!(OutputFormat::parse("xml"), Err("xml".to_string()));
        for name in FORMAT_NAMES {
            assert!(OutputFormat::parse(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn json_pretty_and_compact() {
        let val = json!({"name": "test", "n": [1, 2]});
        assert_eq!(
            fmt(&val, OutputFormat::Json),
            "{\n  \"n\": [\n    1,\n    2\n  ],\n  \"name\": \"test\"\n}"
        );
        let compact = OutputSettings {
            json_style: JsonStyle::Compact,
            ..Default::default()
        };
        assert_eq!(
            fmt_with(&val, OutputFormat::Json, &compact),
            r#"{"n":[1,2],"name":"test"}"#
        );
    }

    #[test]
    fn paginated_json_is_always_compact() {
        let val = json!({"files": [{"id": "1"}]});
        assert_eq!(
            page(&val, OutputFormat::Json, true),
            r#"{"files":[{"id":"1"}]}"#
        );
    }

    #[test]
    fn table_array_of_objects() {
        let val = json!({
            "files": [
                {"id": "1", "name": "hello.txt"},
                {"id": "2", "name": "world.txt"}
            ],
            "nextPageToken": "t"
        });
        insta::assert_snapshot!(fmt(&val, OutputFormat::Table), @r"
        id  name
        ──  ─────────
        1   hello.txt
        2   world.txt
        ");
    }

    #[test]
    fn table_single_object_flattens_nested() {
        let val = json!({"id": "abc", "user": {"displayName": "Alice"}});
        insta::assert_snapshot!(fmt(&val, OutputFormat::Table), @r"
        id                abc
        user.displayName  Alice
        ");
    }

    #[test]
    fn table_nested_objects_in_array() {
        let val = json!([
            {"id": "1", "owner": {"name": "Alice"}},
            {"id": "2", "owner": {"name": "Bob"}}
        ]);
        let out = fmt(&val, OutputFormat::Table);
        assert!(out.contains("owner.name"), "{out}");
        assert!(out.contains("Bob"), "{out}");
    }

    #[test]
    fn table_dry_run_output_keeps_method_and_url() {
        let val = json!({
            "dry_run": true,
            "url": "https://www.googleapis.com/drive/v3/files",
            "method": "GET",
            "query_params": [["pageSize", "5"]],
            "body": null,
            "is_multipart_upload": false,
        });
        let out = fmt(&val, OutputFormat::Table);
        assert!(out.contains("method"), "{out}");
        assert!(out.contains("GET"), "{out}");
        assert!(
            out.contains("https://www.googleapis.com/drive/v3/files"),
            "{out}"
        );
    }

    #[test]
    fn table_multibyte_truncation_does_not_panic() {
        let val = json!([{"col": "😀".repeat(70)}]);
        let out = fmt(&val, OutputFormat::Table);
        assert!(out.contains('…'));
    }

    #[test]
    fn table_empty_list() {
        assert_eq!(fmt(&json!({"files": []}), OutputFormat::Table), "(empty)");
    }

    #[test]
    fn table_columns_selection() {
        let val = json!({"files": [
            {"id": "1", "name": "a", "owner": {"email": "x@y"}},
            {"id": "2", "name": "b", "owner": {"email": "z@y"}}
        ]});
        let settings = OutputSettings {
            columns: Some(vec!["owner.email".into(), "id".into()]),
            ..Default::default()
        };
        insta::assert_snapshot!(fmt_with(&val, OutputFormat::Table, &settings), @r"
        owner.email  id
        ───────────  ──
        x@y          1
        z@y          2
        ");
    }

    #[test]
    fn unknown_column_is_an_error() {
        let val = json!({"files": [{"id": "1"}]});
        let settings = OutputSettings {
            columns: Some(vec!["nmae".into()]),
            ..Default::default()
        };
        let err = render(&val, OutputFormat::Csv, &settings, None).unwrap_err();
        assert!(err.to_string().contains("nmae"), "{err}");
        assert!(err.to_string().contains("Available: id"), "{err}");
    }

    #[test]
    fn csv_array_of_objects() {
        let val = json!({"files": [
            {"id": "1", "name": "hello"},
            {"id": "2", "name": "has,comma"}
        ]});
        assert_eq!(
            fmt(&val, OutputFormat::Csv),
            "id,name\n1,hello\n2,\"has,comma\""
        );
    }

    #[test]
    fn csv_columns_selection() {
        let val = json!([{"id": "1", "name": "a"}, {"id": "2", "name": "b"}]);
        let settings = OutputSettings {
            columns: Some(vec!["name".into()]),
            ..Default::default()
        };
        assert_eq!(fmt_with(&val, OutputFormat::Csv, &settings), "name\na\nb");
    }

    #[test]
    fn csv_array_of_arrays() {
        let val = json!({
            "range": "Sheet1!A1:C3",
            "majorDimension": "ROWS",
            "values": [["Name", "Class"], ["Alexandra", "4. Senior"], ["Q\"uote", "x"]]
        });
        assert_eq!(
            fmt(&val, OutputFormat::Csv),
            "Name,Class\nAlexandra,4. Senior\n\"Q\"\"uote\",x"
        );
    }

    #[test]
    fn csv_flat_scalars() {
        let val = json!(["plain", "has,comma"]);
        assert_eq!(fmt(&val, OutputFormat::Csv), "plain\n\"has,comma\"");
    }

    #[test]
    fn yaml_is_valid_and_quotes_special_strings() {
        let val = json!({
            "kind": "drive#file",
            "url": "https://example.com/a",
            "count": 42,
            "list": ["a", "b"],
            "empty": [],
            "body": "line one\nline two",
            "yes": "yes"
        });
        let out = fmt(&val, OutputFormat::Yaml);
        assert!(!out.starts_with('\n'), "{out}");
        // Round-trip through a YAML parser proves the output is valid and lossless.
        let back: Value = serde_saphyr::from_str(&out).unwrap();
        assert_eq!(back, val, "{out}");
    }

    #[test]
    fn paginated_csv_and_table_headers_only_on_first_page() {
        let val = json!({"files": [{"id": "3", "name": "c.txt"}]});
        assert_eq!(page(&val, OutputFormat::Csv, true), "id,name\n3,c.txt");
        assert_eq!(page(&val, OutputFormat::Csv, false), "3,c.txt");
        assert!(page(&val, OutputFormat::Table, true).contains("──"));
        let cont = page(&val, OutputFormat::Table, false);
        assert!(!cont.contains("──") && cont.contains("c.txt"), "{cont}");
    }

    #[test]
    fn paginated_yaml_has_document_separator() {
        let val = json!({"files": [{"id": "1"}]});
        assert!(page(&val, OutputFormat::Yaml, true).starts_with("---\n"));
        assert!(page(&val, OutputFormat::Yaml, false).starts_with("---\n"));
    }

    #[test]
    fn jq_json_prints_strings_raw_one_per_line() {
        let settings = OutputSettings {
            jq: Some(JqFilter::compile(".files[] | .id, .n").unwrap()),
            ..Default::default()
        };
        let val = json!({"files": [{"id": "a", "n": 1}, {"id": "b", "n": {"x": 2}}]});
        assert_eq!(
            fmt_with(&val, OutputFormat::Json, &settings),
            "a\n1\nb\n{\n  \"x\": 2\n}"
        );
    }

    #[test]
    fn jq_then_table() {
        let settings = OutputSettings {
            jq: Some(JqFilter::compile("[.files[] | {id}]").unwrap()),
            ..Default::default()
        };
        let val = json!({"files": [{"id": "a", "n": 1}]});
        let out = fmt_with(&val, OutputFormat::Table, &settings);
        assert!(out.starts_with("id\n"), "{out}");
        assert!(
            !out.contains('n'.to_string().as_str()) || !out.contains(" n"),
            "{out}"
        );
    }

    #[test]
    fn table_strips_terminal_control_sequences() {
        let val = json!({"files": [{"name": "evil\u{1b}]52;c;aGk=\u{7}\u{1b}[2Jname", "k\u{1b}[31m": "v"}]});
        let out = fmt(&val, OutputFormat::Table);
        assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
        let single = fmt(&json!({"a\u{1b}[1m": "b\u{9b}c"}), OutputFormat::Table);
        assert!(
            !single.contains('\u{1b}') && !single.contains('\u{9b}'),
            "{single:?}"
        );
    }

    #[test]
    fn csv_neutralises_formula_injection() {
        let val = json!([
            {"a": "=HYPERLINK(\"http://x\")", "b": "+1", "c": "-cmd", "d": "@SUM(A1)", "e": -5, "f": "ok"}
        ]);
        let out = fmt(&val, OutputFormat::Csv);
        let row = out.lines().nth(1).unwrap();
        assert_eq!(
            row,
            "\"'=HYPERLINK(\"\"http://x\"\")\",'+1,'-cmd,'@SUM(A1),-5,ok"
        );
        let rows = fmt(&json!({"values": [["=1+1", "\tx"]]}), OutputFormat::Csv);
        assert_eq!(rows, "'=1+1,'\tx");
    }

    #[test]
    fn csv_strips_control_sequences() {
        let out = fmt(&json!([{"a": "x\u{1b}[2Jy"}]), OutputFormat::Csv);
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn yaml_escapes_control_chars_and_newline_keys() {
        let val = json!({"a\nkind": "x", "esc": "\u{1b}[2J", "b": {"c: d": "#e"}});
        let out = fmt(&val, OutputFormat::Yaml);
        assert!(!out.contains('\u{1b}'), "{out:?}");
        let back: Value = serde_saphyr::from_str(&out).unwrap();
        assert_eq!(back, val, "{out}");
    }

    #[test]
    fn jq_raw_strings_are_sanitized_on_a_terminal() {
        let settings = OutputSettings {
            jq: Some(JqFilter::compile(".a").unwrap()),
            terminal: true,
            ..Default::default()
        };
        assert_eq!(
            fmt_with(&json!({"a": "x\u{1b}[2Jy"}), OutputFormat::Json, &settings),
            "x[2Jy"
        );
    }

    #[test]
    fn jq_empty_output_prints_nothing() {
        let settings = OutputSettings {
            jq: Some(JqFilter::compile("empty").unwrap()),
            ..Default::default()
        };
        assert_eq!(fmt_with(&json!(1), OutputFormat::Json, &settings), "");
    }
}
