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

/// Deepest list `nestingLevel` the Docs API produces.
const MAX_NESTING_LEVEL: u64 = 8;

/// Render the document body. `markdown` selects Markdown vs plain text.
pub(super) fn render(document: &Value, markdown: bool) -> String {
    let mut out = String::new();
    if let Some(content) = document.pointer("/body/content").and_then(Value::as_array) {
        render_elements(content, document, markdown, &mut out);
    }
    out
}

fn render_elements(elements: &[Value], document: &Value, markdown: bool, out: &mut String) {
    let mut in_list = false;
    for el in elements {
        let is_item = el.pointer("/paragraph/bullet").is_some();
        // List items are written one per line; a block that follows them
        // needs a blank line, or CommonMark reads a paragraph as a lazy
        // continuation of the last item.
        if markdown && in_list && !is_item && !out.ends_with("\n\n") {
            out.push('\n');
        }
        in_list = is_item;
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
    // Markdown: consecutive runs with the same marks, buffered so they are
    // wrapped once (`*xy*`, not `*x**y*`, which reads as literal asterisks).
    let mut pending: Option<(Marks, String)> = None;
    let flush = |line: &mut String, pending: &mut Option<(Marks, String)>| {
        if let Some((marks, text)) = pending.take() {
            line.push_str(&styled(&text, &marks));
        }
    };
    if let Some(elements) = p.get("elements").and_then(Value::as_array) {
        for e in elements {
            if let Some(run) = e.get("textRun") {
                let text = run.get("content").and_then(Value::as_str).unwrap_or("");
                let text = text.trim_end_matches('\n');
                if markdown {
                    let marks = Marks::of(run.get("textStyle"));
                    match &mut pending {
                        Some((open, buf)) if *open == marks => buf.push_str(text),
                        _ => {
                            flush(&mut line, &mut pending);
                            pending = Some((marks, text.to_string()));
                        }
                    }
                } else {
                    line.push_str(text);
                }
                continue;
            }
            flush(&mut line, &mut pending);
            if e.get("inlineObjectElement").is_some() {
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
    flush(&mut line, &mut pending);
    // A vertical tab is a line break inside the paragraph (Shift+Enter).
    // Markdown needs a hard break (`\` before the newline) to keep it; a
    // trailing one has nothing to break and would leave a literal `\`.
    let line = if markdown {
        escape_line_starts(
            &line
                .trim_end_matches('\u{000b}')
                .replace('\u{000b}', "\\\n"),
        )
    } else {
        line.replace('\u{000b}', "\n")
    };
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
        let marker = |level: u64| {
            if is_ordered(document, list_id, level) {
                "1."
            } else {
                "-"
            }
        };
        // CommonMark nests an item only when it is indented to its parent's
        // content column: 2 under "- ", 3 under "1. ". `nestingLevel` is
        // 0..=8 per the Docs API; the bound keeps a malformed value from
        // producing an unbounded indent.
        let indent: String = (0..level.min(MAX_NESTING_LEVEL))
            .map(|parent| " ".repeat(marker(parent).len() + 1))
            .collect();
        out.push_str(&format!("{indent}{} {line}\n", marker(level)));
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

/// Drop leading indentation and escape block markers that only mean
/// something at the start of a line: `-`/`+` bullets, `=` setext underlines
/// and `1.`/`1)` ordered items.
///
/// Markdown has no indented paragraphs: a line starting with a tab or four
/// spaces is an indented code block, which would show the escaped text
/// verbatim in monospace, so the indentation is removed.
fn escape_line_starts(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            let body = l.trim_start_matches([' ', '\t']);
            let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if body.starts_with(['-', '+', '=']) {
                format!("\\{body}")
            } else if digits > 0 && body[digits..].starts_with(['.', ')']) {
                format!("{}\\{}", &body[..digits], &body[digits..])
            } else {
                body.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Markdown-visible part of a run's `textStyle`.
#[derive(Debug, Default, PartialEq)]
struct Marks {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    mono: bool,
    link: Option<String>,
}

impl Marks {
    fn of(style: Option<&Value>) -> Self {
        let Some(style) = style else {
            return Self::default();
        };
        let on = |k: &str| style.get(k).and_then(Value::as_bool).unwrap_or(false);
        Self {
            bold: on("bold"),
            italic: on("italic"),
            strikethrough: on("strikethrough"),
            mono: style
                .pointer("/weightedFontFamily/fontFamily")
                .and_then(Value::as_str)
                .is_some_and(|f| {
                    let f = f.to_lowercase();
                    f.contains("mono") || f.contains("courier") || f.contains("consolas")
                }),
            link: style
                .pointer("/link/url")
                .and_then(Value::as_str)
                .map(str::to_string),
        }
    }
}

/// Wrap text in Markdown marks, keeping surrounding whitespace outside.
/// Literal text is escaped; code (monospace) text is kept verbatim.
fn styled(text: &str, marks: &Marks) -> String {
    // A code span cannot hold a line break: the hard break written for a
    // vertical tab would be literal text inside it. Wrap each line on its
    // own, so the breaks fall between the spans.
    if marks.mono && text.contains('\u{000b}') {
        return text
            .split('\u{000b}')
            .map(|line| styled(line, marks))
            .collect::<Vec<_>>()
            .join("\u{000b}");
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mut core = if marks.mono {
        format!("`{trimmed}`")
    } else {
        escape_markdown(trimmed)
    };
    if marks.strikethrough {
        core = format!("~~{core}~~");
    }
    if marks.italic {
        core = format!("*{core}*");
    }
    if marks.bold {
        core = format!("**{core}**");
    }
    if let Some(url) = &marks.link {
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
        if c == '|' && backslashes.is_multiple_of(2) {
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

    /// Maximum list nesting depth CommonMark sees in `md`.
    fn list_depth(md: &str) -> usize {
        use pulldown_cmark::{Event, Parser, Tag, TagEnd};
        let (mut depth, mut max) = (0usize, 0usize);
        for event in Parser::new(md) {
            match event {
                Event::Start(Tag::List(_)) => {
                    depth += 1;
                    max = max.max(depth);
                }
                Event::End(TagEnd::List(_)) => depth -= 1,
                _ => {}
            }
        }
        max
    }

    #[test]
    fn nested_list_items_stay_nested_in_markdown() {
        let item = |list: &str, level: u64, text: &str| {
            json!({"paragraph": {"bullet": {"listId": list, "nestingLevel": level},
                "elements": [{"textRun": {"content": format!("{text}\n")}}]}})
        };
        let document = json!({
            "lists": {
                "N": {"listProperties": {"nestingLevels": [
                    {"glyphType": "DECIMAL"}, {"glyphSymbol": "○"}, {"glyphType": "DECIMAL"}
                ]}},
                "B": {"listProperties": {"nestingLevels": [
                    {"glyphSymbol": "●"}, {"glyphType": "DECIMAL"}, {"glyphSymbol": "■"}
                ]}}
            },
            "body": {"content": [
                item("N", 0, "numbered"),
                item("N", 1, "bullet under number"),
                item("N", 2, "number under bullet"),
            ]}
        });
        let md = render(&document, true);
        assert_eq!(list_depth(&md), 3, "nesting lost in:\n{md}");

        let document = json!({
            "lists": document["lists"].clone(),
            "body": {"content": [
                item("B", 0, "bullet"),
                item("B", 1, "number under bullet"),
                item("B", 2, "bullet under number"),
            ]}
        });
        let md = render(&document, true);
        assert_eq!(list_depth(&md), 3, "nesting lost in:\n{md}");
        // Bullet parents keep the two-space indent.
        assert!(md.contains("\n  1. number under bullet\n"), "{md}");
    }

    #[test]
    fn paragraph_after_a_list_is_not_merged_into_the_last_item() {
        let para = |t: &str| json!({"paragraph": {"elements": [{"textRun": {"content": t}}]}});
        let table = json!({"table": {"tableRows": [{"tableCells": [
            {"content": [para("A\n")]}, {"content": [para("B\n")]}
        ]}]}});
        for after in [para("After the list\n"), table] {
            let document = json!({
                "lists": doc()["lists"].clone(),
                "body": {"content": [
                    {"paragraph": {"bullet": {"listId": "L1"},
                        "elements": [{"textRun": {"content": "item\n"}}]}},
                    after,
                ]}
            });
            let md = render(&document, true);
            let (tags, _) = parse_back(&md);
            // The block after the list is a sibling of the list, not part of its item.
            let block = tags
                .iter()
                .any(|t| t == "Paragraph" || t.starts_with("Table"));
            assert!(
                block && tags.iter().filter(|t| t.starts_with("Item")).count() == 1,
                "list item swallowed the next block: {tags:?} from:\n{md}"
            );
        }
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
    fn indented_paragraphs_do_not_become_code_blocks() {
        let para = |t: &str| json!({"paragraph": {"elements": [{"textRun": {"content": t}}]}});
        let document = json!({"body": {"content": [
            para("\tTabbed snake_case\n"),
            para("    Four spaces\n"),
            para("Line one\u{000b}    indented line two\n"),
        ]}});
        let md = render(&document, true);
        let (tags, text) = parse_back(&md);
        assert!(
            tags.iter().all(|t| t == "Paragraph"),
            "indented text parsed as {tags:?} from:\n{md:?}"
        );
        assert_eq!(
            text, "Tabbed snake_case\nFour spaces\nLine one\nindented line two\n",
            "from:\n{md:?}"
        );
    }

    #[test]
    fn text_rendering() {
        let txt = render(&doc(), false);
        assert!(txt.starts_with("Title\nSome bold link\n"));
        assert!(txt.contains("A\tB|C\n"));
    }

    #[test]
    fn styled_keeps_whitespace_outside_marks() {
        let marks = |style: Value| Marks::of(Some(&style));
        assert_eq!(styled(" x ", &marks(json!({"italic": true}))), " *x* ");
        assert_eq!(
            styled(
                "f()",
                &marks(json!({"weightedFontFamily": {"fontFamily": "Courier New"}}))
            ),
            "`f()`"
        );
    }

    #[test]
    fn line_breaks_stay_hard_breaks_in_markdown() {
        use pulldown_cmark::{Event, Parser};
        let para = |t: &str| json!({"paragraph": {"elements": [{"textRun": {"content": t}}]}});
        let item = json!({"paragraph": {
            "elements": [{"textRun": {"content": "item\u{000b}- more\n"}}],
            "bullet": {"listId": "l"}
        }});
        let document = json!({"body": {"content": [
            para("one\u{000b}two\n"),
            para("trailing\u{000b}\n"),
            item,
        ]}});
        let md = render(&document, true);
        let events: Vec<Event<'_>> = Parser::new(&md).collect();
        let hard = events
            .iter()
            .filter(|e| matches!(e, Event::HardBreak))
            .count();
        let soft = events
            .iter()
            .filter(|e| matches!(e, Event::SoftBreak))
            .count();
        assert_eq!((hard, soft), (2, 0), "from:\n{md:?}");
        assert!(!md.contains("trailing\\"), "dangling break in:\n{md:?}");
        // `docs +write --markdown` turns the item's hard break back into a line break.
        let written = super::super::markdown::render(&md)
            .unwrap()
            .to_requests(1, false);
        let text = written[0]["insertText"]["text"].as_str().unwrap();
        assert!(text.ends_with("item\u{b}- more"), "{text:?}");
        assert_eq!(
            render(&document, false),
            "one\ntwo\ntrailing\n\n- item\n- more\n"
        );
    }

    /// A line break (Shift+Enter) inside monospace text, such as a code
    /// snippet, must not put the hard break's `\` inside a code span, where
    /// it is literal text.
    #[test]
    fn line_breaks_in_code_do_not_add_a_literal_backslash() {
        use pulldown_cmark::{Event, Parser};
        let mono = json!({"weightedFontFamily": {"fontFamily": "Courier New"}});
        let document = json!({"body": {"content": [{"paragraph": {"elements": [
            {"textRun": {"content": "let a = 1;\u{b}let b = 2;\u{b}", "textStyle": mono}},
            {"textRun": {"content": "\n"}}
        ]}}]}});
        let md = render(&document, true);
        let events: Vec<Event<'_>> = Parser::new(&md).collect();
        let code: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::Code(c) => Some(c.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(code, ["let a = 1;", "let b = 2;"], "from:\n{md:?}");
        let hard = events
            .iter()
            .filter(|e| matches!(e, Event::HardBreak))
            .count();
        assert_eq!(hard, 1, "from:\n{md:?}");
        assert_eq!(md, "`let a = 1;`\\\n`let b = 2;`\n\n");
    }

    #[test]
    fn adjacent_runs_with_the_same_marks_share_one_span() {
        // Docs splits runs on any style change (colour, underline, size), so
        // one bold or italic phrase often arrives as several runs.
        let document = json!({"body": {"content": [{"paragraph": {"elements": [
            {"textRun": {"content": "Read ", "textStyle": {}}},
            {"textRun": {"content": "x", "textStyle": {"italic": true}}},
            {"textRun": {"content": "y", "textStyle": {"italic": true, "underline": true}}},
            {"textRun": {"content": " and ", "textStyle": {}}},
            {"textRun": {"content": "im", "textStyle": {"bold": true}}},
            {"textRun": {"content": "portant", "textStyle": {"bold": true, "fontSize": {"magnitude": 14, "unit": "PT"}}}},
            {"textRun": {"content": " ", "textStyle": {}}},
            {"textRun": {"content": "f(", "textStyle": {"weightedFontFamily": {"fontFamily": "Courier New"}}}},
            {"textRun": {"content": ")", "textStyle": {"weightedFontFamily": {"fontFamily": "Courier New"}, "bold": false}}},
            {"textRun": {"content": "\n"}}
        ]}}]}});
        let md = render(&document, true);
        let (tags, text) = parse_back(&md);
        // Code span text is reported in `tags`, not `text`.
        assert_eq!(text, "Read xy and important \n", "from:\n{md}");
        assert_eq!(
            tags,
            ["Paragraph", "Emphasis", "Strong", "Code(Borrowed(\"f()\"))"],
            "from:\n{md}"
        );
    }
}
