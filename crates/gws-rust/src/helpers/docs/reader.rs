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
                line.push_str(name);
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
                    line.push_str(&format!("[{title}]({uri})"));
                } else {
                    line.push_str(title);
                }
            }
        }
    }
    let line = line.replace('\u{000b}', "\n");
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

/// Wrap a run in Markdown marks, keeping surrounding whitespace outside.
fn styled(text: &str, style: Option<&Value>) -> String {
    let Some(style) = style else {
        return text.to_string();
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
        trimmed.to_string()
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
    if markdown {
        joined.replace('|', "\\|")
    } else {
        joined
    }
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
