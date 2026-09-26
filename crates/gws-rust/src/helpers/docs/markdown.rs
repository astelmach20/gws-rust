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

//! Markdown → Google Docs `batchUpdate` requests.
//!
//! The Markdown is flattened into plain text plus paragraph and character
//! style ranges (in UTF-16 code units, which is how Docs indexes text), then
//! turned into one `insertText` followed by style requests. Bullets are
//! created last and in reverse document order, because `createParagraphBullets`
//! removes the leading tabs used to express nesting and would otherwise shift
//! the indices of later requests. Each list item is a single Docs paragraph:
//! its hard breaks, later paragraphs and code blocks continue it after a line
//! break (U+000B) so they neither become bullets nor split the list.

use crate::error::GwsError;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

const MONOSPACE: &str = "Courier New";
/// A line break inside a paragraph (Shift+Enter in the Docs editor).
const LINE_BREAK: &str = "\u{000b}";

#[derive(Debug, Clone, PartialEq)]
enum ParaKind {
    Normal,
    Heading(u8),
    Bullet { ordered: bool, list_id: usize },
    Code,
    Quote,
    Rule,
}

#[derive(Debug, Clone, PartialEq)]
struct Para {
    start: u64,
    end: u64,
    kind: ParaKind,
}

#[derive(Debug, Clone, PartialEq)]
enum SpanStyle {
    Bold,
    Italic,
    Strike,
    Code,
    Link(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Span {
    start: u64,
    end: u64,
    style: SpanStyle,
}

/// Flattened Markdown.
#[derive(Debug, Default)]
pub(super) struct Rendered {
    text: String,
    paras: Vec<Para>,
    spans: Vec<Span>,
}

fn utf16_len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

struct Builder {
    out: Rendered,
    len: u64,
    para_start: Option<(u64, ParaKind)>,
    open_spans: Vec<(u64, SpanStyle)>,
    /// (ordered, list_id) for each open list.
    lists: Vec<(bool, usize)>,
    next_list_id: usize,
    in_quote: usize,
    in_code_block: bool,
    /// The open code block belongs to a list item and continues its paragraph.
    code_in_item: bool,
}

impl Builder {
    fn push(&mut self, s: &str) {
        self.out.text.push_str(s);
        self.len += utf16_len(s);
    }

    fn begin_para(&mut self, kind: ParaKind) {
        if self.para_start.is_none() {
            let kind = if kind == ParaKind::Normal && self.in_quote > 0 {
                ParaKind::Quote
            } else {
                kind
            };
            self.para_start = Some((self.len, kind));
        }
    }

    fn end_para(&mut self) {
        if let Some((start, kind)) = self.para_start.take() {
            self.push("\n");
            self.out.paras.push(Para {
                start,
                end: self.len,
                kind,
            });
        }
    }

    fn list_item_kind(&self) -> ParaKind {
        match self.lists.last() {
            Some(&(ordered, list_id)) => ParaKind::Bullet { ordered, list_id },
            None => ParaKind::Normal,
        }
    }

    /// Continue the current list item on a new line of the same Docs
    /// paragraph (a vertical tab, which Docs renders as a line break).
    ///
    /// A list item is one Docs paragraph: a separate paragraph would either
    /// become a bullet of its own or, as a plain paragraph, split the list in
    /// two and restart its numbering. If the item's paragraph was already
    /// closed (by a nested list or a paragraph end), the last bullet
    /// paragraph is reopened.
    fn continue_item(&mut self) {
        match &self.para_start {
            Some((start, _)) => {
                if self.len > *start {
                    self.push(LINE_BREAK);
                }
            }
            None => {
                let reopen = self.out.paras.last().is_some_and(|p| {
                    p.end == self.len && matches!(p.kind, ParaKind::Bullet { .. })
                });
                if let (true, Some(p)) = (reopen, self.out.paras.pop()) {
                    // Drop the paragraph's '\n' (one UTF-16 unit).
                    self.out.text.pop();
                    self.len -= 1;
                    self.para_start = Some((p.start, p.kind));
                    self.push(LINE_BREAK);
                } else {
                    let kind = self.list_item_kind();
                    self.begin_para(kind);
                }
            }
        }
    }

    fn in_list_item(&self) -> bool {
        !self.lists.is_empty()
    }

    fn text(&mut self, t: &str) {
        if self.para_start.is_none() {
            let kind = self.list_item_kind();
            self.begin_para(kind);
        }
        self.push(t);
    }

    fn open_span(&mut self, style: SpanStyle) {
        self.open_spans.push((self.len, style));
    }

    fn close_span(&mut self) {
        if let Some((start, style)) = self.open_spans.pop()
            && self.len > start
        {
            self.out.spans.push(Span {
                start,
                end: self.len,
                style,
            });
        }
    }
}

fn unsupported(what: &str) -> GwsError {
    GwsError::Validation(format!(
        "Markdown {what} are not supported by docs +write; remove them or use the raw documents batchUpdate API"
    ))
}

/// Parse Markdown into flattened text and style ranges.
pub(super) fn render(md: &str) -> Result<Rendered, GwsError> {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut b = Builder {
        out: Rendered::default(),
        len: 0,
        para_start: None,
        open_spans: Vec::new(),
        lists: Vec::new(),
        next_list_id: 0,
        in_quote: 0,
        in_code_block: false,
        code_in_item: false,
    };
    for event in Parser::new_ext(md, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if b.in_list_item() && b.para_start.is_none() {
                        // A later paragraph of the same list item.
                        b.continue_item();
                    } else {
                        let kind = b.list_item_kind();
                        b.begin_para(kind);
                    }
                }
                Tag::Heading { level, .. } => {
                    b.end_para();
                    b.begin_para(ParaKind::Heading(heading_level(level)));
                }
                Tag::BlockQuote(_) => {
                    b.end_para();
                    b.in_quote += 1;
                }
                Tag::CodeBlock(_) => {
                    b.code_in_item = b.in_list_item();
                    if !b.code_in_item {
                        b.end_para();
                    }
                    b.in_code_block = true;
                }
                Tag::List(start) => {
                    // A nested list ends the parent item's first paragraph.
                    b.end_para();
                    let id = match b.lists.first() {
                        // Nested lists share the top-level list so Docs nests them.
                        Some(&(_, top)) => top,
                        None => {
                            b.next_list_id += 1;
                            b.next_list_id
                        }
                    };
                    let ordered = match b.lists.first() {
                        Some(&(top_ordered, _)) => top_ordered,
                        None => start.is_some(),
                    };
                    b.lists.push((ordered, id));
                }
                Tag::Item => {
                    b.end_para();
                    let kind = b.list_item_kind();
                    b.begin_para(kind);
                    let depth = b.lists.len().saturating_sub(1);
                    for _ in 0..depth {
                        b.push("\t");
                    }
                }
                Tag::Emphasis => b.open_span(SpanStyle::Italic),
                Tag::Strong => b.open_span(SpanStyle::Bold),
                Tag::Strikethrough => b.open_span(SpanStyle::Strike),
                Tag::Link { dest_url, .. } => b.open_span(SpanStyle::Link(dest_url.to_string())),
                Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => {
                    return Err(unsupported("tables"));
                }
                Tag::Image { .. } => return Err(unsupported("images")),
                Tag::HtmlBlock => return Err(unsupported("HTML blocks")),
                Tag::FootnoteDefinition(_) => return Err(unsupported("footnotes")),
                other => {
                    return Err(GwsError::Validation(format!(
                        "Unsupported Markdown element for docs +write: {other:?}"
                    )));
                }
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => b.end_para(),
                TagEnd::BlockQuote(_) => {
                    b.end_para();
                    b.in_quote = b.in_quote.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    b.in_code_block = false;
                    b.code_in_item = false;
                }
                TagEnd::List(_) => {
                    b.end_para();
                    b.lists.pop();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    b.close_span();
                }
                _ => {}
            },
            Event::Text(t) => {
                if b.code_in_item {
                    // Code lines continue the list item's paragraph.
                    for line in t.split_inclusive('\n') {
                        b.continue_item();
                        let content = line.strip_suffix('\n').unwrap_or(line);
                        let start = b.len;
                        b.push(content);
                        if b.len > start {
                            b.out.spans.push(Span {
                                start,
                                end: b.len,
                                style: SpanStyle::Code,
                            });
                        }
                    }
                } else if b.in_code_block {
                    // One Docs paragraph per code line.
                    for line in t.split_inclusive('\n') {
                        b.begin_para(ParaKind::Code);
                        let content = line.strip_suffix('\n').unwrap_or(line);
                        let start = b.len;
                        b.push(content);
                        if b.len > start {
                            b.out.spans.push(Span {
                                start,
                                end: b.len,
                                style: SpanStyle::Code,
                            });
                        }
                        if line.ends_with('\n') {
                            b.end_para();
                        }
                    }
                } else {
                    b.text(&t);
                }
            }
            Event::Code(t) => {
                b.text("");
                let start = b.len;
                b.push(&t);
                b.out.spans.push(Span {
                    start,
                    end: b.len,
                    style: SpanStyle::Code,
                });
            }
            Event::SoftBreak => b.text(" "),
            Event::HardBreak => {
                let kind = b
                    .para_start
                    .as_ref()
                    .map(|(_, k)| k.clone())
                    .unwrap_or(ParaKind::Normal);
                if matches!(kind, ParaKind::Bullet { .. }) {
                    // Continuation lines of a list item stay in its paragraph.
                    b.continue_item();
                } else {
                    b.end_para();
                    b.begin_para(kind);
                }
            }
            Event::Rule => {
                b.end_para();
                b.begin_para(ParaKind::Rule);
                b.end_para();
            }
            Event::TaskListMarker(done) => b.text(if done { "☑ " } else { "☐ " }),
            Event::Html(_) | Event::InlineHtml(_) => return Err(unsupported("inline HTML tags")),
            Event::FootnoteReference(_) => return Err(unsupported("footnotes")),
            Event::InlineMath(_) | Event::DisplayMath(_) => return Err(unsupported("math blocks")),
        }
    }
    b.end_para();
    if b.out.text.is_empty() {
        return Err(GwsError::Validation(
            "Markdown produced no text to insert".into(),
        ));
    }
    Ok(b.out)
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn range(start: u64, end: u64) -> Value {
    json!({ "startIndex": start, "endIndex": end })
}

impl Rendered {
    /// Build batchUpdate requests inserting this content at `index`.
    ///
    /// When `paragraph_break` is set a newline is inserted first so the content
    /// starts in a new paragraph. The final newline of the Markdown is dropped:
    /// the document's own trailing newline terminates the last paragraph.
    pub(super) fn to_requests(&self, index: u64, paragraph_break: bool) -> Vec<Value> {
        let prefix = u64::from(paragraph_break);
        let base = index + prefix;
        let mut text = String::new();
        if paragraph_break {
            text.push('\n');
        }
        text.push_str(self.text.strip_suffix('\n').unwrap_or(&self.text));
        let total_end = base + utf16_len(&self.text);

        let mut requests = vec![json!({
            "insertText": { "location": { "index": index }, "text": text }
        })];
        // Reset inherited styling over the whole inserted range.
        requests.push(json!({
            "deleteParagraphBullets": { "range": range(base, total_end) }
        }));
        requests.push(json!({
            "updateTextStyle": {
                "range": range(base, total_end),
                "textStyle": {},
                "fields": "bold,italic,strikethrough,underline,link,weightedFontFamily"
            }
        }));

        for p in &self.paras {
            let r = range(base + p.start, base + p.end);
            let (named, extra): (&str, Option<(Value, &str)>) = match &p.kind {
                ParaKind::Heading(n) => (heading_style(*n), None),
                ParaKind::Quote => (
                    "NORMAL_TEXT",
                    Some((
                        json!({ "indentStart": { "magnitude": 36, "unit": "PT" }, "indentFirstLine": { "magnitude": 36, "unit": "PT" } }),
                        "indentStart,indentFirstLine",
                    )),
                ),
                ParaKind::Rule => (
                    "NORMAL_TEXT",
                    Some((
                        json!({ "borderBottom": {
                            "color": { "color": { "rgbColor": { "red": 0.6, "green": 0.6, "blue": 0.6 } } },
                            "width": { "magnitude": 1, "unit": "PT" },
                            "padding": { "magnitude": 1, "unit": "PT" },
                            "dashStyle": "SOLID"
                        } }),
                        "borderBottom",
                    )),
                ),
                _ => ("NORMAL_TEXT", None),
            };
            requests.push(json!({
                "updateParagraphStyle": {
                    "range": r,
                    "paragraphStyle": { "namedStyleType": named },
                    "fields": "namedStyleType"
                }
            }));
            if let Some((style, fields)) = extra {
                requests.push(json!({
                    "updateParagraphStyle": { "range": r, "paragraphStyle": style, "fields": fields }
                }));
            }
        }

        for s in &self.spans {
            let (style, fields) = match &s.style {
                SpanStyle::Bold => (json!({ "bold": true }), "bold"),
                SpanStyle::Italic => (json!({ "italic": true }), "italic"),
                SpanStyle::Strike => (json!({ "strikethrough": true }), "strikethrough"),
                SpanStyle::Code => (
                    json!({ "weightedFontFamily": { "fontFamily": MONOSPACE } }),
                    "weightedFontFamily",
                ),
                SpanStyle::Link(url) => (json!({ "link": { "url": url } }), "link"),
            };
            requests.push(json!({
                "updateTextStyle": {
                    "range": range(base + s.start, base + s.end),
                    "textStyle": style,
                    "fields": fields
                }
            }));
        }

        // Group consecutive bullet paragraphs of the same list, then emit the
        // groups last-to-first so tab removal doesn't shift earlier ranges.
        let mut groups: Vec<(u64, u64, bool)> = Vec::new();
        let mut current: Option<(u64, u64, bool, usize)> = None;
        for p in &self.paras {
            match (&p.kind, current) {
                (ParaKind::Bullet { list_id, .. }, Some((s, e, o, id)))
                    if *list_id == id && e == p.start =>
                {
                    current = Some((s, p.end, o, id));
                }
                (ParaKind::Bullet { ordered, list_id }, prev) => {
                    if let Some((s, e, o, _)) = prev {
                        groups.push((s, e, o));
                    }
                    current = Some((p.start, p.end, *ordered, *list_id));
                }
                (_, Some((s, e, o, _))) => {
                    groups.push((s, e, o));
                    current = None;
                }
                (_, None) => {}
            }
        }
        if let Some((s, e, o, _)) = current {
            groups.push((s, e, o));
        }
        for (s, e, ordered) in groups.into_iter().rev() {
            requests.push(json!({
                "createParagraphBullets": {
                    "range": range(base + s, base + e),
                    "bulletPreset": if ordered {
                        "NUMBERED_DECIMAL_ALPHA_ROMAN"
                    } else {
                        "BULLET_DISC_CIRCLE_SQUARE"
                    }
                }
            }));
        }
        requests
    }
}

fn heading_style(n: u8) -> &'static str {
    match n {
        1 => "HEADING_1",
        2 => "HEADING_2",
        3 => "HEADING_3",
        4 => "HEADING_4",
        5 => "HEADING_5",
        _ => "HEADING_6",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn find<'a>(reqs: &'a [Value], key: &str) -> Vec<&'a Value> {
        reqs.iter().filter_map(|r| r.get(key)).collect()
    }

    #[test]
    fn heading_and_inline_styles() {
        let r = render("# Title\n\nSome **bold** and *it* `code` [l](https://x.test).").unwrap();
        assert_eq!(r.text, "Title\nSome bold and it code l.\n");
        assert_eq!(
            r.paras[0],
            Para {
                start: 0,
                end: 6,
                kind: ParaKind::Heading(1)
            }
        );
        let bold = r.spans.iter().find(|s| s.style == SpanStyle::Bold).unwrap();
        assert_eq!(&r.text[bold.start as usize..bold.end as usize], "bold");
        let link = r
            .spans
            .iter()
            .find(|s| matches!(s.style, SpanStyle::Link(_)))
            .unwrap();
        assert_eq!(&r.text[link.start as usize..link.end as usize], "l");
    }

    #[test]
    fn requests_offset_and_drop_final_newline() {
        let r = render("# T").unwrap();
        let reqs = r.to_requests(10, true);
        assert_eq!(reqs[0]["insertText"]["text"], "\nT");
        assert_eq!(reqs[0]["insertText"]["location"]["index"], 10);
        let para = find(&reqs, "updateParagraphStyle");
        assert_eq!(para[0]["range"], json!({"startIndex": 11, "endIndex": 13}));
        assert_eq!(para[0]["paragraphStyle"]["namedStyleType"], "HEADING_1");
    }

    #[test]
    fn utf16_offsets() {
        // "😀" is two UTF-16 code units.
        let r = render("😀 **b**").unwrap();
        let bold = r.spans.iter().find(|s| s.style == SpanStyle::Bold).unwrap();
        assert_eq!((bold.start, bold.end), (3, 4));
    }

    #[test]
    fn nested_lists_use_tabs_and_single_bullet_group() {
        let r = render("- a\n  - b\n- c\n\n1. x\n2. y\n").unwrap();
        assert_eq!(r.text, "a\n\tb\nc\nx\ny\n");
        let reqs = r.to_requests(1, false);
        let bullets = find(&reqs, "createParagraphBullets");
        assert_eq!(bullets.len(), 2);
        // Emitted last-to-first: numbered list first.
        assert_eq!(bullets[0]["bulletPreset"], "NUMBERED_DECIMAL_ALPHA_ROMAN");
        assert_eq!(bullets[1]["bulletPreset"], "BULLET_DISC_CIRCLE_SQUARE");
        assert_eq!(bullets[1]["range"], json!({"startIndex": 1, "endIndex": 8}));
    }

    #[test]
    fn code_block_lines_are_monospace_paragraphs() {
        let r = render("```\nlet x = 1;\nlet y = 2;\n```\n").unwrap();
        assert_eq!(r.text, "let x = 1;\nlet y = 2;\n");
        assert_eq!(r.paras.len(), 2);
        assert!(r.paras.iter().all(|p| p.kind == ParaKind::Code));
        assert_eq!(
            r.spans
                .iter()
                .filter(|s| s.style == SpanStyle::Code)
                .count(),
            2
        );
    }

    #[test]
    fn quote_and_rule() {
        let r = render("> quoted\n\n---\n\nafter").unwrap();
        assert_eq!(r.paras[0].kind, ParaKind::Quote);
        assert_eq!(r.paras[1].kind, ParaKind::Rule);
        let reqs = r.to_requests(1, false);
        assert!(
            find(&reqs, "updateParagraphStyle")
                .iter()
                .any(|p| p["fields"] == "borderBottom")
        );
    }

    #[test]
    fn unsupported_constructs_fail_loudly() {
        assert!(render("| a | b |\n|---|---|\n| 1 | 2 |").is_err());
        assert!(render("![img](x.png)").is_err());
        assert!(render("<div>x</div>").is_err());
        assert!(render("").is_err());
    }

    fn bullet_ranges(r: &Rendered) -> Vec<Value> {
        find(&r.to_requests(1, false), "createParagraphBullets")
            .into_iter()
            .map(|b| b["range"].clone())
            .collect()
    }

    #[test]
    fn list_item_continuation_paragraph_is_not_a_new_item() {
        // A second paragraph inside item 1 must not become item 2.
        let r = render("1. a\n\n   more\n2. b\n").unwrap();
        assert_eq!(
            bullet_ranges(&r),
            vec![json!({"startIndex": 1, "endIndex": 10})]
        );
        assert_eq!(r.paras.len(), 2);
        assert_eq!(r.text, "a\u{b}more\nb\n");
    }

    #[test]
    fn list_item_hard_break_keeps_one_numbered_list() {
        // A hard break inside an item must not split the list (which
        // restarts the numbering of the following items at 1).
        let r = render("1. a  \n   more\n2. b\n").unwrap();
        assert_eq!(
            bullet_ranges(&r),
            vec![json!({"startIndex": 1, "endIndex": 10})]
        );
        assert_eq!(r.text, "a\u{b}more\nb\n");
    }

    #[test]
    fn list_item_code_block_keeps_one_numbered_list() {
        let r = render("1. a\n\n   ```\n   x\n   y\n   ```\n2. b\n").unwrap();
        assert_eq!(
            bullet_ranges(&r),
            vec![json!({"startIndex": 1, "endIndex": 9})]
        );
        assert_eq!(r.text, "a\u{b}x\u{b}y\nb\n");
        let code: Vec<_> = r
            .spans
            .iter()
            .filter(|s| s.style == SpanStyle::Code)
            .map(|s| (s.start, s.end))
            .collect();
        assert_eq!(code, vec![(2, 3), (4, 5)]);
    }

    #[test]
    fn nested_item_continuation_stays_in_the_nested_item() {
        let r = render("- a\n  - b\n\n    cont\n- c\n").unwrap();
        assert_eq!(r.paras.len(), 3);
        assert_eq!(r.text, "a\n\tb\u{b}cont\nc\n");
    }

    #[test]
    fn hard_break_outside_lists_still_splits_paragraphs() {
        let r = render("one  \ntwo").unwrap();
        assert_eq!(r.text, "one\ntwo\n");
        assert_eq!(r.paras.len(), 2);
    }

    #[test]
    fn task_list_markers() {
        let r = render("- [ ] todo\n- [x] done").unwrap();
        assert_eq!(r.text, "☐ todo\n☑ done\n");
    }
}
