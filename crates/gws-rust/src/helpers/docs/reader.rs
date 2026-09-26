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

//! Render a Docs API `Document` as plain text or Markdown.

use serde_json::Value;

/// Render the document body. `markdown` selects Markdown vs plain text.
pub(super) fn render(document: &Value, markdown: bool) -> String {
    let mut out = String::new();
    if let Some(content) = document.pointer("/body/content").and_then(Value::as_array) {
        render_elements(content, document, markdown, &mut out);
    }
    out
}

fn render_elements(elements: &[Value], document: &Value, markdown: bool, out: &mut String) {
    for el in elements {
        if let Some(p) = el.get("paragraph") {
            render_paragraph(p, document, markdown, out);
        } else if let Some(t) = el.get("table") {
            render_table(t, document, markdown, out);
        }
        // sectionBreak and tableOfContents carry no body text of their own.
    }
}

fn is_ordered(document: &Value, list_id: &str, level: u64) -> bool {
    document
        .pointer(&format!(
            "/lists/{list_id}/listProperties/nestingLevels/{level}"
        ))
        .map(|nl| {
            nl.get("glyphType")
                .and_then(Value::as_str)
                .is_some_and(|g| !matches!(g, "GLYPH_TYPE_UNSPECIFIED" | "NONE"))
                && nl.get("glyphSymbol").is_none()
        })
        .unwrap_or(false)
}

fn render_paragraph(p: &Value, document: &Value, markdown: bool, out: &mut String) {
    let mut line = String::new();
    if let Some(elements) = p.get("elements").and_then(Value::as_array) {
        for e in elements {
            if let Some(run) = e.get("textRun") {
                let text = run.get("content").and_then(Value::as_str).unwrap_or("");
                let text = text.trim_end_matches('\n');
                if markdown {
                    line.push_str(&styled(text, run.get("textStyle")));
                } else {
                    line.push_str(text);
                }
            } else if e.get("inlineObjectElement").is_some() {
                line.push_str("[image]");
            } else if let Some(person) = e.get("person") {
                let name = person
                    .pointer("/personProperties/name")
                    .or_else(|| person.pointer("/personProperties/email"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if markdown {
                    line.push_str(&escape_markdown(name));
                } else {
                    line.push_str(name);
                }
            } else if let Some(link) = e.get("richLink") {
                let title = link
                    .pointer("/richLinkProperties/title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let uri = link
                    .pointer("/richLinkProperties/uri")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if markdown {
                    line.push_str(&format!("[{}]({uri})", escape_markdown(title)));
                } else {
                    line.push_str(title);
                }
            }
        }
    }
    let mut line = line.replace('\u{000b}', "\n");
    if markdown {
        line = escape_line_starts(&line);
    }
    let style = p
        .pointer("/paragraphStyle/namedStyleType")
        .and_then(Value::as_str)
        .unwrap_or("NORMAL_TEXT");

    if let Some(bullet) = p.get("bullet") {
        let level = bullet
            .get("nestingLevel")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let list_id = bullet.get("listId").and_then(Value::as_str).unwrap_or("");
        // `nestingLevel` is 0..=8 per the Docs API; a value beyond usize (only
        // possible on 32-bit targets) renders unindented rather than failing.
        let indent = "  ".repeat(usize::try_from(level).unwrap_or(0));
        let marker = if is_ordered(document, list_id, level) {
            "1."
        } else {
            "-"
        };
        out.push_str(&format!("{indent}{marker} {line}\n"));
        return;
    }
    if markdown {
        let prefix = match style {
            "TITLE" | "HEADING_1" => "# ",
            "SUBTITLE" | "HEADING_2" => "## ",
            "HEADING_3" => "### ",
            "HEADING_4" => "#### ",
            "HEADING_5" => "##### ",
            "HEADING_6" => "###### ",
            _ => "",
        };
        if line.trim().is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("{prefix}{line}\n\n"));
        }
    } else {
        out.push_str(&line);
        out.push('\n');
    }
}

/// Backslash-escape the characters that would otherwise turn literal
/// document text into Markdown syntax (emphasis, code, links, HTML, entities,
/// headings, quotes, tables).
fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '~' | '|' | '&'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape block markers that only mean something at the start of a line:
/// `-`/`+` bullets, `=` setext underlines and `1.`/`1)` ordered items.
fn escape_line_starts(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            let body = l.trim_start_matches(' ');
            let indent = &l[..l.len() - body.len()];
            let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if body.starts_with(['-', '+', '=']) {
                format!("{indent}\\{body}")
            } else if digits > 0 && body[digits..].starts_with(['.', ')']) {
                format!("{indent}{}\\{}", &body[..digits], &body[digits..])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wrap a run in Markdown marks, keeping surrounding whitespace outside.
/// Literal text is escaped; code (monospace) runs are kept verbatim.
fn styled(text: &str, style: Option<&Value>) -> String {
    let Some(style) = style else {
        return escape_markdown(text);
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let on = |k: &str| style.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mono = style
        .pointer("/weightedFontFamily/fontFamily")
        .and_then(Value::as_str)
        .is_some_and(|f| {
            let f = f.to_lowercase();
            f.contains("mono") || f.contains("courier") || f.contains("consolas")
        });
    let mut core = if mono {
        format!("`{trimmed}`")
    } else {
        escape_markdown(trimmed)
    };
    if on("strikethrough") {
        core = format!("~~{core}~~");
    }
    if on("italic") {
        core = format!("*{core}*");
    }
    if on("bold") {
        core = format!("**{core}**");
    }
    if let Some(url) = style.pointer("/link/url").and_then(Value::as_str) {
        core = format!("[{core}]({url})");
    }
    format!("{lead}{core}{trail}")
}

fn cell_text(cell: &Value, document: &Value, markdown: bool) -> String {
    let mut s = String::new();
    if let Some(content) = cell.get("content").and_then(Value::as_array) {
        render_elements(content, document, markdown, &mut s);
    }
    let joined = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !markdown {
        return joined;
    }
    // Literal text is already escaped by its runs; a `|` still bare here is in
    // a code span, where GFM tables also need it escaped to stay in the cell.
    let mut out = String::with_capacity(joined.len());
    let mut backslashes = 0usize;
    for c in joined.chars() {
        if c == '|' && backslashes % 2 == 0 {
            out.push('\\');
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
        out.push(c);
    }
    out
}

fn render_table(table: &Value, document: &Value, markdown: bool, out: &mut String) {
    let rows = table
        .get("tableRows")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = row
            .get("tableCells")
            .and_then(Value::as_array)
            .map(|cells| {
                cells
                    .iter()
                    .map(|c| cell_text(c, document, markdown))
                    .collect()
            })
            .unwrap_or_default();
        if markdown {
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
            if i == 0 {
                out.push_str(&format!("|{}\n", " --- |".repeat(cells.len().max(1))));
            }
        } else {
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
    }
    if markdown {
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "lists": {"L1": {"listProperties": {"nestingLevels": [
                {"glyphSymbol": "●"}
            ]}}, "L2": {"listProperties": {"nestingLevels": [
                {"glyphType": "DECIMAL"}
            ]}}},
            "body": {"content": [
                {"sectionBreak": {}},
                {"paragraph": {"paragraphStyle": {"namedStyleType": "HEADING_1"},
                    "elements": [{"textRun": {"content": "Title\n"}}]}},
                {"paragraph": {"elements": [
                    {"textRun": {"content": "Some "}},
                    {"textRun": {"content": "bold ", "textStyle": {"bold": true}}},
                    {"textRun": {"content": "link", "textStyle": {"link": {"url": "https://x.test"}}}},
                    {"textRun": {"content": "\n"}}
                ]}},
                {"paragraph": {"bullet": {"listId": "L1"},
                    "elements": [{"textRun": {"content": "item\n"}}]}},
                {"paragraph": {"bullet": {"listId": "L2"},
                    "elements": [{"textRun": {"content": "first\n"}}]}},
                {"table": {"tableRows": [
                    {"tableCells": [
                        {"content": [{"paragraph": {"elements": [{"textRun": {"content": "A\n"}}]}}]},
                        {"content": [{"paragraph": {"elements": [{"textRun": {"content": "B|C\n"}}]}}]}
                    ]}
                ]}}
            ]}
        })
    }

    #[test]
    fn markdown_rendering() {
        let md = render(&doc(), true);
        assert!(md.contains("# Title\n"));
        assert!(md.contains("Some **bold** [link](https://x.test)"));
        assert!(md.contains("- item\n"));
        assert!(md.contains("1. first\n"));
        assert!(md.contains("| A | B\\|C |\n| --- | --- |"));
    }

    #[test]
    fn markdown_table_cells_escape_pipes_once() {
        let cell = |run: Value| json!({"content": [{"paragraph": {"elements": [run]}}]});
        let document = json!({"body": {"content": [{"table": {"tableRows": [{"tableCells": [
            cell(json!({"textRun": {"content": "a|b\n"}})),
            cell(json!({"textRun": {"content": "x|y", "textStyle": {"weightedFontFamily": {"fontFamily": "Courier New"}}}})),
        ]}]}}]}});
        assert!(render(&document, true).starts_with("| a\\|b | `x\\|y` |\n"));
    }

    /// Parse Markdown back and describe it as (block structure, text, inline
    /// marks), so a test can check what a Markdown reader actually sees.
    fn parse_back(md: &str) -> (Vec<String>, String) {
        use pulldown_cmark::{Event, Options, Parser};
        let mut tags = Vec::new();
        let mut text = String::new();
        let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
        for event in Parser::new_ext(md, options) {
            match event {
                Event::Start(tag) => tags.push(format!("{tag:?}")),
                Event::Text(t) => text.push_str(&t),
                Event::SoftBreak | Event::HardBreak => text.push('\n'),
                Event::End(pulldown_cmark::TagEnd::Paragraph) => text.push('\n'),
                other @ (Event::Html(_) | Event::InlineHtml(_) | Event::Code(_)) => {
                    tags.push(format!("{other:?}"));
                }
                _ => {}
            }
        }
        (tags, text)
    }

    #[test]
    fn markdown_escapes_literal_markdown_syntax() {
        let para = |t: &str| json!({"paragraph": {"elements": [{"textRun": {"content": t}}]}});
        let lines = [
            "# of seats: 12",
            "1. This sentence is not a list item.",
            "- 5 degrees overnight",
            "+ extra",
            "Call f(*args, **kwargs) on snake_case_names",
            "See [draft] and <b>bold</b> & ~~old~~ `code`",
            "> not a quote",
            "C:\\temp\\*",
        ];
        let body: Vec<Value> = lines.iter().map(|l| para(&format!("{l}\n"))).collect();
        let document = json!({"body": {"content": body}});
        let md = render(&document, true);
        let (tags, text) = parse_back(&md);
        assert!(
            tags.iter().all(|t| t == "Paragraph"),
            "literal text parsed as Markdown structure {tags:?} from:\n{md}"
        );
        assert_eq!(text, format!("{}\n", lines.join("\n")), "from:\n{md}");
        // Real styling still renders, and the plain-text form is untouched.
        let styled_doc = json!({"body": {"content": [{"paragraph": {"elements": [
            {"textRun": {"content": "a_b", "textStyle": {"bold": true}}},
            {"textRun": {"content": " x*y", "textStyle": {"weightedFontFamily": {"fontFamily": "Roboto Mono"}}}},
            {"textRun": {"content": "\n"}}
        ]}}]}});
        assert_eq!(render(&styled_doc, true), "**a\\_b** `x*y`\n\n");
        assert_eq!(render(&document, false), format!("{}\n", lines.join("\n")));
    }

    #[test]
    fn text_rendering() {
        let txt = render(&doc(), false);
        assert!(txt.starts_with("Title\nSome bold link\n"));
        assert!(txt.contains("A\tB|C\n"));
    }

    #[test]
    fn styled_keeps_whitespace_outside_marks() {
        assert_eq!(styled(" x ", Some(&json!({"italic": true}))), " *x* ");
        assert_eq!(
            styled(
                "f()",
                Some(&json!({"weightedFontFamily": {"fontFamily": "Courier New"}}))
            ),
            "`f()`"
        );
    }
}
