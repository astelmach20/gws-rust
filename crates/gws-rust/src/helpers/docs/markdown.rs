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
/// Paragraph style fields cleared on inserted content, so it does not keep
/// the layout of the paragraph it was inserted into.
const PARAGRAPH_RESET_FIELDS: &str = "alignment,lineSpacing,spaceAbove,spaceBelow,\
indentFirstLine,indentStart,indentEnd,borderTop,borderBottom,borderLeft,borderRight,\
borderBetween,shading";

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

/// Characters `insertText` silently drops: the control characters
/// U+0000-U+0008 and U+000C-U+001F, and the Basic Multilingual Plane Private
/// Use Area U+E000-U+F8FF. Tab, newline and the line break (U+000B) are kept.
fn docs_strips(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{c}'..='\u{1f}' | '\u{e000}'..='\u{f8ff}')
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
    /// The next code text in a list item starts a new code line. pulldown-cmark
    /// can split one line over several `Text` events (it does for CRLF input),
    /// so a line break is added only where a line really starts.
    code_line_start: bool,
    /// Where the current list item's content starts, after its nesting tabs.
    item_body: u64,
}

impl Builder {
    fn push(&mut self, s: &str) {
        // Docs strips these from inserted text; counting them would shift
        // every later style range off the text it was meant for.
        let kept: String = s.chars().filter(|&c| !docs_strips(c)).collect();
        self.out.text.push_str(&kept);
        self.len += utf16_len(&kept);
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
                // The current item's nesting tabs are not content.
                if self.len > (*start).max(self.item_body) {
                    self.push(LINE_BREAK);
                }
            }
            None => {
                let reopen = self.out.paras.last().is_some_and(|p| {
                    p.end == self.len && matches!(p.kind, ParaKind::Bullet { .. })
                });
                // Pop only when reopening: any other last paragraph (a
                // heading inside the item) must be kept.
                if reopen && let Some(p) = self.out.paras.pop() {
                    // Drop the paragraph's '\n' (one UTF-16 unit).
                    self.out.text.pop();
                    self.len -= 1;
                    // A paragraph holding only the item's nesting tabs (the
                    // item opened with a quote) has no line to break.
                    if self.len > p.start.max(self.item_body) {
                        self.push(LINE_BREAK);
                    }
                    self.para_start = Some((p.start, p.kind));
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
        code_line_start: false,
        item_body: 0,
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
                    b.code_line_start = true;
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
                    b.item_body = b.len;
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
                        if b.code_line_start {
                            b.continue_item();
                            b.code_line_start = false;
                        }
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
                        b.code_line_start = line.ends_with('\n');
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
                if b.len > start {
                    b.out.spans.push(Span {
                        start,
                        end: b.len,
                        style: SpanStyle::Code,
                    });
                }
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

        let mut requests = Vec::new();
        // Content that is one empty paragraph (a lone rule, an empty heading
        // or list item) has no text before the newline it shares with the
        // document; Docs rejects an insertText with no text, so the styles
        // below apply to the paragraph already at `index`.
        if !text.is_empty() {
            requests.push(json!({
                "insertText": { "location": { "index": index }, "text": text }
            }));
        }
        // Reset styling inherited from the paragraph the content lands in
        // (its text style, and paragraph styles such as a quote's indent or a
        // rule's border) over the whole inserted range.
        requests.push(json!({
            "deleteParagraphBullets": { "range": range(base, total_end) }
        }));
        requests.push(json!({
            "updateTextStyle": {
                "range": range(base, total_end),
                "textStyle": {},
                "fields": "*"
            }
        }));
        requests.push(json!({
            "updateParagraphStyle": {
                "range": range(base, total_end),
                "paragraphStyle": {},
                "fields": PARAGRAPH_RESET_FIELDS
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
        // [0] resets inherited paragraph styling, [1] is the heading.
        assert_eq!(para[0]["range"], json!({"startIndex": 11, "endIndex": 13}));
        assert_eq!(para[1]["range"], json!({"startIndex": 11, "endIndex": 13}));
        assert_eq!(para[1]["paragraphStyle"]["namedStyleType"], "HEADING_1");
    }

    #[test]
    fn single_empty_paragraph_sends_no_empty_insert() {
        // A lone rule, empty heading or empty item is one empty paragraph:
        // its only text is the newline that the document's own final newline
        // replaces. Docs rejects an insertText without text, so none is sent
        // and the styles apply to the paragraph at `index`.
        for (md, named) in [("---", "NORMAL_TEXT"), ("#", "HEADING_1"), ("-", "")] {
            let reqs = render(md).unwrap().to_requests(1, false);
            assert!(find(&reqs, "insertText").is_empty(), "{md}: {reqs:?}");
            for key in ["deleteParagraphBullets", "updateTextStyle"] {
                assert_eq!(
                    find(&reqs, key)[0]["range"],
                    json!({"startIndex": 1, "endIndex": 2}),
                    "{md}"
                );
            }
            if !named.is_empty() {
                assert_eq!(
                    find(&reqs, "updateParagraphStyle")[1]["paragraphStyle"]["namedStyleType"],
                    named
                );
            }
        }
        let reqs = render("-").unwrap().to_requests(1, false);
        assert_eq!(
            find(&reqs, "createParagraphBullets")[0]["range"],
            json!({"startIndex": 1, "endIndex": 2})
        );
        // After a paragraph with text, the break itself is still inserted.
        let reqs = render("---").unwrap().to_requests(5, true);
        assert_eq!(reqs[0]["insertText"]["text"], "\n");
    }

    #[test]
    fn characters_docs_strips_do_not_shift_style_ranges() {
        // Docs drops control characters and Private Use Area characters from
        // inserted text, so they must not count toward later offsets.
        let r = render("a\u{c}\u{1b}\u{f8ff} **b** `\u{e000}`").unwrap();
        assert_eq!(r.text, "a b \n");
        let bold = r.spans.iter().find(|s| s.style == SpanStyle::Bold).unwrap();
        assert_eq!((bold.start, bold.end), (2, 3));
        // A code span left empty is not styled (an empty range is rejected).
        assert!(r.spans.iter().all(|s| s.style != SpanStyle::Code));
        // Tabs and line breaks are kept.
        let r = render("x\ty").unwrap();
        assert_eq!(r.text, "x\ty\n");
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
    fn list_item_code_block_with_crlf_line_endings_has_no_blank_lines() {
        // pulldown-cmark splits CRLF code lines over several text events
        // ("x", "\ny", "\n"); each event must not start a new line.
        for md in [
            "1. a\r\n\r\n   ```\r\n   x\r\n   y\r\n   ```\r\n2. b\r\n",
            "1. a\r\n\r\n       x\r\n       y\r\n2. b\r\n",
        ] {
            let r = render(md).unwrap();
            assert_eq!(r.text, "a\u{b}x\u{b}y\nb\n", "{md:?}");
            let code: Vec<_> = r
                .spans
                .iter()
                .filter(|s| s.style == SpanStyle::Code)
                .map(|s| (s.start, s.end))
                .collect();
            assert_eq!(code, vec![(2, 3), (4, 5)], "{md:?}");
        }
        // A blank code line is still kept.
        let r = render("- a\r\n\r\n  ```\r\n  x\r\n\r\n  y\r\n  ```\r\n").unwrap();
        assert_eq!(r.text, "a\u{b}x\u{b}\u{b}y\n");
    }

    #[test]
    fn nested_item_continuation_stays_in_the_nested_item() {
        let r = render("- a\n  - b\n\n    cont\n- c\n").unwrap();
        assert_eq!(r.paras.len(), 3);
        assert_eq!(r.text, "a\n\tb\u{b}cont\nc\n");
    }

    #[test]
    fn nested_item_starting_with_code_block_has_no_empty_first_line() {
        // The nesting tabs are not content: the code must start the item
        // instead of following a line break after them.
        let r = render("- a\n  - ```\n    x\n    ```\n").unwrap();
        assert_eq!(r.text, "a\n\tx\n");
        let code: Vec<_> = r
            .spans
            .iter()
            .filter(|s| s.style == SpanStyle::Code)
            .map(|s| (s.start, s.end))
            .collect();
        assert_eq!(code, vec![(3, 4)]);
        // A later code line still continues the item.
        let r = render("- a\n  - ```\n    x\n    y\n    ```\n").unwrap();
        assert_eq!(r.text, "a\n\tx\u{b}y\n");
    }

    #[test]
    fn item_starting_with_block_quote_has_no_empty_first_line() {
        // The quote closes the item's (still empty) paragraph; its text must
        // start the item, not follow a line break.
        let r = render("- > quoted\n- b\n").unwrap();
        assert_eq!(r.text, "quoted\nb\n");
        assert_eq!(r.paras.len(), 2);
        let r = render("- a\n  - > quoted\n").unwrap();
        assert_eq!(r.text, "a\n\tquoted\n");
        // A quote after the item's own text still continues it on a new line.
        let r = render("- a\n\n  > quoted\n").unwrap();
        assert_eq!(r.text, "a\u{b}quoted\n");
    }

    #[test]
    fn heading_inside_a_list_item_keeps_its_paragraph() {
        // The paragraph after the heading used to drop the heading's
        // paragraph, leaving its text unstyled and outside every paragraph.
        let r = render("- # h\n\n  more").unwrap();
        assert_eq!(r.text, "\nh\nmore\n");
        let heading = Para {
            start: 1,
            end: 3,
            kind: ParaKind::Heading(1),
        };
        assert!(r.paras.contains(&heading), "{:?}", r.paras);
        let ends: Vec<u64> = r.paras.iter().map(|p| p.end).collect();
        let starts: Vec<u64> = r.paras.iter().map(|p| p.start).collect();
        assert_eq!(starts, [0, 1, 3], "paragraphs must cover the text");
        assert_eq!(ends, [1, 3, 8]);
    }

    #[test]
    fn hard_break_outside_lists_still_splits_paragraphs() {
        let r = render("one  \ntwo").unwrap();
        assert_eq!(r.text, "one\ntwo\n");
        assert_eq!(r.paras.len(), 2);
    }

    /// A minimal Docs body: UTF-16 units starting at index 1, each with a
    /// text style, and a paragraph style per paragraph-ending newline.
    struct SimDoc {
        units: Vec<u16>,
        text_style: Vec<serde_json::Map<String, Value>>,
        para_style: Vec<serde_json::Map<String, Value>>,
    }

    impl SimDoc {
        /// One paragraph `text` (ending in '\n') with the given styles.
        fn new(text: &str, text_style: Value, para_style: Value) -> Self {
            let units: Vec<u16> = text.encode_utf16().collect();
            let n = units.len();
            Self {
                units,
                text_style: vec![text_style.as_object().unwrap().clone(); n],
                para_style: vec![para_style.as_object().unwrap().clone(); n],
            }
        }

        /// The paragraph-ending newline at or after position `i`.
        fn para_end(&self, i: usize) -> usize {
            (i..self.units.len())
                .find(|&j| self.units[j] == u16::from(b'\n'))
                .unwrap()
        }

        fn update(target: &mut serde_json::Map<String, Value>, style: &Value, fields: &str) {
            if fields == "*" {
                target.clear();
            }
            for f in fields.split(',').filter(|f| *f != "*") {
                match style.get(f) {
                    Some(v) => target.insert(f.to_string(), v.clone()),
                    None => target.remove(f),
                };
            }
            if fields == "*" {
                for (k, v) in style.as_object().unwrap() {
                    target.insert(k.clone(), v.clone());
                }
            }
        }

        /// Apply the requests this test cares about (text and styles).
        fn apply(&mut self, reqs: &[Value]) {
            for r in reqs {
                if let Some(ins) = r.get("insertText") {
                    let at = ins["location"]["index"].as_u64().unwrap() as usize - 1;
                    let new: Vec<u16> = ins["text"].as_str().unwrap().encode_utf16().collect();
                    // Inserted text takes the style of the text before it, and
                    // new paragraphs split off the paragraph they land in.
                    let ts = self.text_style[at.saturating_sub(1)].clone();
                    let ps = self.para_style[self.para_end(at)].clone();
                    for (k, u) in new.into_iter().enumerate() {
                        self.units.insert(at + k, u);
                        self.text_style.insert(at + k, ts.clone());
                        self.para_style.insert(at + k, ps.clone());
                    }
                } else if let Some(u) = r.get("updateTextStyle") {
                    let s = u["range"]["startIndex"].as_u64().unwrap() as usize - 1;
                    let e = u["range"]["endIndex"].as_u64().unwrap() as usize - 1;
                    for t in &mut self.text_style[s..e] {
                        Self::update(t, &u["textStyle"], u["fields"].as_str().unwrap());
                    }
                } else if let Some(u) = r.get("updateParagraphStyle") {
                    let s = u["range"]["startIndex"].as_u64().unwrap() as usize - 1;
                    let e = u["range"]["endIndex"].as_u64().unwrap() as usize - 1;
                    let mut i = s;
                    while i < e {
                        let end = self.para_end(i);
                        Self::update(
                            &mut self.para_style[end],
                            &u["paragraphStyle"],
                            u["fields"].as_str().unwrap(),
                        );
                        i = end + 1;
                    }
                }
            }
        }

        /// Text, paragraph style and first unit's text style of each paragraph.
        fn paragraphs(&self) -> Vec<(String, Value, Value)> {
            let mut out = Vec::new();
            let mut start = 0;
            while start < self.units.len() {
                let end = self.para_end(start);
                out.push((
                    String::from_utf16(&self.units[start..=end]).unwrap(),
                    Value::Object(self.para_style[end].clone()),
                    Value::Object(self.text_style[start].clone()),
                ));
                start = end + 1;
            }
            out
        }
    }

    #[test]
    fn appended_markdown_does_not_inherit_quote_indent_or_text_style() {
        // The document ends with a quote written by an earlier +write
        // --markdown, in large red text.
        let mut doc = SimDoc::new(
            "Quoted\n",
            json!({"fontSize": {"magnitude": 24, "unit": "PT"},
                   "foregroundColor": {"color": {"rgbColor": {"red": 1}}}}),
            json!({"namedStyleType": "NORMAL_TEXT",
                   "indentStart": {"magnitude": 36, "unit": "PT"},
                   "indentFirstLine": {"magnitude": 36, "unit": "PT"}}),
        );
        // append_point: endIndex 8, last paragraph has text.
        doc.apply(&render("plain").unwrap().to_requests(7, true));
        let paras = doc.paragraphs();
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[1].0, "plain\n");
        assert_eq!(paras[1].1, json!({"namedStyleType": "NORMAL_TEXT"}));
        assert_eq!(paras[1].2, json!({}));
        // The existing paragraph keeps its own styling.
        assert!(paras[0].1.get("indentStart").is_some());
    }

    #[test]
    fn markdown_after_a_rule_does_not_inherit_its_border() {
        // An earlier +write --markdown ending in `---` leaves an empty last
        // paragraph with a bottom border; the next write lands in it.
        let mut doc = SimDoc::new(
            "\n",
            json!({}),
            json!({"namedStyleType": "NORMAL_TEXT", "borderBottom": {"width": {"magnitude": 1, "unit": "PT"}}}),
        );
        doc.apply(&render("text").unwrap().to_requests(1, false));
        let paras = doc.paragraphs();
        assert_eq!(paras[0].0, "text\n");
        assert_eq!(paras[0].1, json!({"namedStyleType": "NORMAL_TEXT"}));
    }

    #[test]
    fn task_list_markers() {
        let r = render("- [ ] todo\n- [x] done").unwrap();
        assert_eq!(r.text, "☐ todo\n☑ done\n");
    }
}
