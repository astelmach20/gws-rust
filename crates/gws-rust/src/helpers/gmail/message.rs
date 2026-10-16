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

//! Parsing of Gmail API message resources (headers, MIME payload walk).

use super::prelude::*;

/// Metadata for an attachment or inline image from the original message's MIME payload.
///
/// Binary data is usually NOT stored here: it is fetched separately via
/// `fetch_original_parts` using the attachment ID. Small parts may carry their
/// data inline in the API response instead.
#[derive(Debug, Clone)]
pub(super) struct OriginalPart {
    /// Filename from the MIME part. Synthesized as `"part-{index}.{ext}"` when absent.
    pub filename: String,
    /// MIME content type (e.g., `"image/png"`, `"application/pdf"`).
    pub content_type: String,
    /// Size in bytes from the Gmail API `body.size` field.
    pub size: u64,
    /// Where the part's bytes come from.
    pub data: PartData,
    /// Content-ID for inline images (bare, no angle brackets).
    /// When present, the part is an inline image referenced via `cid:` URLs in the HTML body.
    /// When absent, the part is a regular file attachment.
    pub content_id: Option<String>,
}

/// Source of an original part's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PartData {
    /// Fetch with `users.messages.attachments.get`.
    AttachmentId(String),
    /// Base64url data included directly in the message resource.
    Inline(String),
}

impl OriginalPart {
    /// The Gmail attachment ID, when the bytes must be fetched separately.
    #[cfg(test)]
    pub fn attachment_id(&self) -> Option<&str> {
        match &self.data {
            PartData::AttachmentId(id) => Some(id),
            PartData::Inline(_) => None,
        }
    }

    /// Whether this part is an inline image (has a Content-ID and is not explicitly
    /// `Content-Disposition: attachment`) vs a regular file attachment.
    pub fn is_inline(&self) -> bool {
        self.content_id.is_some()
    }

    /// Whether this part is an image embedded in the HTML body: inline (see
    /// [`Self::is_inline`]) and `image/*`.
    ///
    /// Only images count: some clients (Apple Mail) send ordinary files such
    /// as PDFs with `Content-Disposition: inline` and a Content-ID, and those
    /// are attachments the reader expects to get.
    pub fn is_inline_image(&self) -> bool {
        self.is_inline()
            && self
                .content_type
                .get(..6)
                .is_some_and(|t| t.eq_ignore_ascii_case("image/"))
    }
}

/// A parsed Gmail message fetched via the API, used as context for reply/forward.
///
/// `from` is always populated — `parse_original_message` returns an error when
/// `From` is missing. `body_text` always has a value — it falls back to the
/// message snippet when no `text/plain` MIME part is found. Semantically optional
/// fields (`cc`, `reply_to`, `date`, `body_html`) use `Option` so the compiler
/// enforces absence checks. A whitespace-only `text/plain` part yields to a
/// non-blank `text/html` part rendered as text.
#[derive(Default, Serialize)]
pub(super) struct OriginalMessage {
    pub thread_id: Option<String>,
    /// Bare message ID (no angle brackets), e.g. `"abc@example.com"`; empty
    /// when the message has no `Message-ID` header.
    pub message_id: String,
    /// Bare message IDs (no angle brackets) forming the references chain.
    pub references: Vec<String>,
    /// Bare message IDs from the `In-Reply-To` header. Used to start the
    /// references chain when the message has no `References` header.
    #[serde(skip_serializing)]
    pub in_reply_to: Vec<String>,
    pub from: Mailbox,
    /// Multiple Reply-To addresses are allowed per RFC 5322.
    pub reply_to: Option<Vec<Mailbox>>,
    pub to: Vec<Mailbox>,
    pub cc: Option<Vec<Mailbox>>,
    pub subject: String,
    pub date: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
    /// Attachments and inline images from the original MIME payload (metadata only).
    /// Binary data is fetched separately via `fetch_original_parts`.
    #[serde(skip_serializing)]
    pub parts: Vec<OriginalPart>,
}

impl OriginalMessage {
    /// Placeholder used for `--dry-run` to avoid requiring auth/network.
    pub(super) fn dry_run_placeholder(message_id: &str) -> Self {
        Self {
            thread_id: Some(format!("thread-{message_id}")),
            message_id: format!("{message_id}@example.com"),
            from: Mailbox::parse("sender@example.com"),
            to: vec![Mailbox::parse("you@example.com")],
            subject: "Original subject".to_string(),
            date: Some("Thu, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original message body".to_string(),
            body_html: Some("<p>Original message body</p>".to_string()),
            ..Default::default()
        }
    }
}

/// Raw header values extracted from the Gmail API payload, before parsing into
/// structured types. Intermediate step: JSON headers → this → `OriginalMessage`.
#[derive(Default)]
struct ParsedMessageHeaders {
    from: String,
    reply_to: String,
    to: String,
    cc: String,
    subject: String,
    date: String,
    message_id: String,
    references: String,
    in_reply_to: String,
}

fn append_header_value(existing: &mut String, value: &str) {
    if !existing.is_empty() {
        existing.push(' ');
    }
    existing.push_str(value);
}

fn append_address_list_header_value(existing: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }

    if !existing.is_empty() {
        existing.push_str(", ");
    }
    existing.push_str(value);
}

fn parse_message_headers(headers: &[Value]) -> ParsedMessageHeaders {
    let mut parsed = ParsedMessageHeaders::default();

    for header in headers {
        let name = header.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = header.get("value").and_then(|v| v.as_str()).unwrap_or("");

        // RFC 5322 §1.2.2: header field names are case-insensitive.
        match name.to_ascii_lowercase().as_str() {
            "from" => parsed.from = value.to_string(),
            "reply-to" => append_address_list_header_value(&mut parsed.reply_to, value),
            "to" => append_address_list_header_value(&mut parsed.to, value),
            "cc" => append_address_list_header_value(&mut parsed.cc, value),
            "subject" => parsed.subject = value.to_string(),
            "date" => parsed.date = value.to_string(),
            "message-id" => parsed.message_id = value.to_string(),
            "references" => append_header_value(&mut parsed.references, value),
            "in-reply-to" => append_header_value(&mut parsed.in_reply_to, value),
            _ => {}
        }
    }

    parsed
}

/// Convert an empty string to `None`, or apply `f` to the non-empty string.
fn non_empty_then<T>(s: &str, f: impl FnOnce(&str) -> T) -> Option<T> {
    if s.is_empty() { None } else { Some(f(s)) }
}

/// Convert an empty slice to `None`, non-empty to `Some(slice)`.
pub(super) fn non_empty_slice<T>(s: &[T]) -> Option<&[T]> {
    if s.is_empty() { None } else { Some(s) }
}

pub(super) fn parse_original_message(msg: &Value) -> Result<OriginalMessage, GwsError> {
    let thread_id = msg
        .get("threadId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let snippet = msg
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let parsed_headers = msg
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array())
        .map(|headers| parse_message_headers(headers))
        .unwrap_or_default();

    if parsed_headers.from.is_empty() {
        return Err(GwsError::other("Message is missing From header"));
    }

    // A message without a Message-ID can still be replied to or forwarded:
    // the outgoing message then has no In-Reply-To (RFC 5322 §3.6.4), and
    // Gmail keeps it in the thread by thread ID.
    let message_id = parse_msg_ids(&parsed_headers.message_id)
        .into_iter()
        .next()
        .unwrap_or_default();

    let PayloadContents {
        body_text: extracted_text,
        body_html,
        parts: original_parts,
    } = match msg.get("payload") {
        Some(payload) => extract_payload_contents(payload)?,
        None => PayloadContents::default(),
    };

    // Prefer the text/plain part; otherwise render the HTML part as text;
    // only when the message has neither fall back to the API snippet, which
    // Gmail returns HTML-escaped. A whitespace-only text/plain alternative is
    // a stub, not the body, so it yields to a non-blank HTML part.
    let html_is_blank = body_html.as_deref().is_none_or(|h| h.trim().is_empty());
    let body_text = match extracted_text {
        Some(text) if html_is_blank || !text.trim().is_empty() => text,
        _ => match &body_html {
            Some(html) => html_to_text(html)?,
            None => {
                tracing::warn!(
                    "message has no text/plain or text/html body part; using the API snippet, which may be truncated"
                );
                snippet_to_text(&snippet)?
            }
        },
    };

    let references = parse_msg_ids(&parsed_headers.references);
    let in_reply_to = parse_msg_ids(&parsed_headers.in_reply_to);

    // A Reply-To that names no mailbox (only an empty group such as
    // `undisclosed-recipients:;`, or nothing parseable) gives the reply no
    // address, so the reply goes to From as if there were no Reply-To.
    let reply_to = non_empty_then(&parsed_headers.reply_to, Mailbox::parse_header_list)
        .filter(|mailboxes| !mailboxes.is_empty());
    let cc = non_empty_then(&parsed_headers.cc, Mailbox::parse_header_list);
    let date = Some(parsed_headers.date).filter(|s| !s.is_empty());

    Ok(OriginalMessage {
        thread_id,
        message_id,
        references,
        in_reply_to,
        from: Mailbox::parse(&parsed_headers.from),
        reply_to,
        to: Mailbox::parse_header_list(&parsed_headers.to),
        cc,
        subject: parsed_headers.subject,
        date,
        body_text,
        body_html,
        parts: original_parts,
    })
}

/// Everything extracted from the MIME payload in a single recursive pass:
/// the plain text body, HTML body, and attachment/inline part metadata.
#[derive(Default)]
struct PayloadContents {
    body_text: Option<String>,
    body_html: Option<String>,
    parts: Vec<OriginalPart>,
}

/// Base64url engine that accepts both padded and unpadded input, since the
/// Gmail API returns unpadded data for some resources (e.g. attachments).
const BASE64URL_LENIENT: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

/// Decode Gmail base64url data (padding optional).
pub(super) fn decode_base64url(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64URL_LENIENT.decode(data)
}

/// Decode a base64url-encoded text body part in the charset its
/// `Content-Type` header names.
///
/// The Gmail API undoes the Content-Transfer-Encoding but not the charset,
/// so a `charset=iso-8859-1` body arrives as Latin-1 bytes. Invalid base64
/// violates the API contract and is an error. A missing charset means UTF-8
/// (US-ASCII is a subset). An unknown charset, or bytes that are invalid in
/// the named one, are decoded lossily with an explicit warning, since the
/// text is still useful for quoting and reading.
fn decode_text_body(
    data: &str,
    content_type: Option<&str>,
    mime_label: &str,
) -> Result<String, GwsError> {
    let decoded = decode_base64url(data).map_err(|e| {
        GwsError::other(format!("{mime_label} body has invalid base64url data: {e}"))
    })?;
    let charset = content_type.and_then(|ct| content_type_param(ct, "charset"));
    let encoding = match charset.as_deref() {
        // US-ASCII is a subset of UTF-8. (encoding_rs maps the label to
        // windows-1252, which would garble UTF-8 text mislabeled as ASCII.)
        None => encoding_rs::UTF_8,
        Some(label)
            if label.eq_ignore_ascii_case("us-ascii") || label.eq_ignore_ascii_case("ascii") =>
        {
            encoding_rs::UTF_8
        }
        Some(label) => match encoding_rs::Encoding::for_label(label.as_bytes()) {
            // encoding_rs maps the labels it cannot decode (ISO-2022-KR,
            // HZ-GB-2312, ISO-2022-CN) to the replacement encoding, which
            // turns the whole body into a single U+FFFD. UTF-8 at least
            // keeps its ASCII text.
            Some(encoding) if encoding != encoding_rs::REPLACEMENT => encoding,
            _ => {
                tracing::warn!(
                    "{mime_label} body has unknown or unsupported charset {:?}; decoding it as UTF-8",
                    sanitize_for_terminal(label)
                );
                encoding_rs::UTF_8
            }
        },
    };
    // A byte-order mark is not text: it is removed, and a UTF-8 or UTF-16
    // BOM names the encoding (the endianness of a `charset=utf-16` body is
    // only known from its BOM).
    let (text, used, had_errors) = encoding.decode(&decoded);
    if had_errors {
        tracing::warn!(
            "{mime_label} body is not valid {}; invalid bytes were replaced with U+FFFD",
            used.name()
        );
    }
    Ok(text.into_owned())
}

/// The value of parameter `name` (case-insensitive) in a MIME header such as
/// `text/plain; charset="iso-8859-1"`, unquoted. Semicolons inside quoted
/// strings do not split parameters.
fn content_type_param(header: &str, name: &str) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    for c in header.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => {
                current.push(c);
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            ';' if !in_quotes => segments.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    segments.push(current);
    segments.iter().skip(1).find_map(|segment| {
        let (key, value) = segment.split_once('=')?;
        if !key.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        let value = value.trim();
        let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(quoted) => {
                let mut out = String::with_capacity(quoted.len());
                let mut chars = quoted.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        out.extend(chars.next());
                    } else {
                        out.push(c);
                    }
                }
                out
            }
            None => value.to_string(),
        };
        Some(value).filter(|v| !v.is_empty())
    })
}

/// Synthesize a filename from the part index and MIME type when no filename is present.
/// e.g., `"image/png"` at index 1 → `"part-1.png"`.
fn synthesize_filename(part_index: usize, mime_type: &str) -> String {
    let ext = mime_type
        .split('/')
        .nth(1)
        .map(|sub| match sub {
            "jpeg" => "jpg",
            "plain" => "txt",
            "svg+xml" => "svg",
            "octet-stream" => "bin",
            other => other,
        })
        .unwrap_or("bin");
    format!("part-{part_index}.{ext}")
}

/// Sanitize a remote filename: strip ASCII control characters and fall back to
/// a synthesized name if the result is empty. Unlike `--attach` (where we reject
/// bad paths), remote filenames are sender-controlled and should not fail the operation.
fn sanitize_remote_filename(raw: &str, part_index: usize, mime_type: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| !c.is_ascii_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        synthesize_filename(part_index, mime_type)
    } else {
        cleaned.to_string()
    }
}

/// Get a header value from a MIME part's headers array, case-insensitive.
pub(super) fn get_part_header<'a>(part: &'a Value, name: &str) -> Option<&'a str> {
    part.get("headers")
        .and_then(|h| h.as_array())
        .and_then(|headers| {
            headers.iter().find_map(|h| {
                let n = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if n.eq_ignore_ascii_case(name) {
                    h.get("value").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
        })
}

/// Whether `part`'s `Content-Disposition` is `attachment` (in any case, after
/// any leading whitespace). The body walk and the file listing must agree on
/// this, or a part is fetched as the body and listed as a file at once.
fn disposition_is_attachment(part: &Value) -> bool {
    get_part_header(part, "Content-Disposition").is_some_and(|d| {
        d.trim_start()
            .get(.."attachment".len())
            .is_some_and(|word| word.eq_ignore_ascii_case("attachment"))
    })
}

/// The attachment ID of a body text part whose bytes Gmail returns by
/// reference instead of inline, which it does for large bodies.
///
/// These are the parts [`extract_payload_contents`] would take as the body if
/// their data were inline: an unnamed `text/plain` or `text/html` part with
/// no Content-ID that is not marked as an attachment.
fn referenced_body_attachment_id(part: &Value) -> Option<&str> {
    let mime_type = part.get("mimeType").and_then(Value::as_str).unwrap_or("");
    if !(mime_type.eq_ignore_ascii_case("text/plain")
        || mime_type.eq_ignore_ascii_case("text/html"))
    {
        return None;
    }
    let body = part.get("body")?;
    if body.get("data").and_then(Value::as_str).is_some() {
        return None;
    }
    let filename = part.get("filename").and_then(Value::as_str).unwrap_or("");
    if !filename.is_empty()
        || get_part_header(part, "Content-ID").is_some()
        || disposition_is_attachment(part)
    {
        return None;
    }
    body.get("attachmentId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// Whether the body walk looks inside `part`: not a part with its own bytes
/// (an attachment, or an embedded message Gmail exposes as one file), and not
/// an embedded `message/rfc822`, whose text is another message's body.
fn walks_into(part: &Value) -> bool {
    let has_attachment_id = part
        .get("body")
        .and_then(|b| b.get("attachmentId"))
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty());
    let mime_type = part.get("mimeType").and_then(Value::as_str).unwrap_or("");
    !has_attachment_id && !mime_type.eq_ignore_ascii_case("message/rfc822")
}

/// Attachment IDs of the body text parts of `payload` that Gmail returned by
/// reference (see [`referenced_body_attachment_id`]), in payload order.
pub(super) fn referenced_body_attachment_ids(payload: &Value) -> Vec<String> {
    fn walk(part: &Value, ids: &mut Vec<String>) {
        if let Some(id) = referenced_body_attachment_id(part) {
            ids.push(id.to_string());
        } else if walks_into(part)
            && let Some(children) = part.get("parts").and_then(Value::as_array)
        {
            for child in children {
                walk(child, ids);
            }
        }
    }
    let mut ids = Vec::new();
    walk(payload, &mut ids);
    ids
}

/// Put the fetched bytes of every body text part Gmail returned by reference
/// into the part's `body.data`, as if Gmail had sent them inline, so the
/// payload walk reads them as the body instead of listing them as files.
pub(super) fn inline_referenced_bodies(
    payload: &mut Value,
    fetched: &std::collections::HashMap<String, Vec<u8>>,
) -> Result<(), GwsError> {
    if let Some(id) = referenced_body_attachment_id(payload) {
        let bytes = fetched.get(id).ok_or_else(|| {
            GwsError::other(format!("body part {id} was not fetched before parsing"))
        })?;
        let data = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        if let Some(body) = payload.get_mut("body").and_then(Value::as_object_mut) {
            body.remove("attachmentId");
            body.insert("data".to_string(), Value::String(data));
        }
    } else if walks_into(payload)
        && let Some(children) = payload.get_mut("parts").and_then(Value::as_array_mut)
    {
        for child in children {
            inline_referenced_bodies(child, fetched)?;
        }
    }
    Ok(())
}

/// Walk the MIME payload tree in a single pass, collecting the text body, HTML body,
/// and metadata for all attachment/inline parts.
fn extract_payload_contents(payload: &Value) -> Result<PayloadContents, GwsError> {
    let mut contents = PayloadContents::default();
    let bodies = extract_payload_recursive(payload, &mut contents, &mut 0, false)?;
    contents.body_text = bodies.text;
    contents.body_html = bodies.html;
    Ok(contents)
}

/// The text and HTML body found under one MIME node.
#[derive(Default)]
struct NodeBodies {
    text: Option<String>,
    html: Option<String>,
}

impl NodeBodies {
    /// Combine the bodies of a container's children.
    ///
    /// The children of `multipart/alternative` are renderings of the same
    /// content, so the first one of each kind wins. The children of any
    /// other multipart (`mixed`, `related`, ...) are shown one after the
    /// other, so a body split around inline attachments (as Apple Mail
    /// sends it) is joined in order instead of losing every piece but the
    /// first.
    fn combine(children: Vec<NodeBodies>, alternative: bool) -> NodeBodies {
        let mut combined = NodeBodies::default();
        for child in children {
            if alternative {
                combined.text = combined.text.or(child.text);
                combined.html = combined.html.or(child.html);
            } else {
                append_body(&mut combined.text, child.text, "\n");
                append_body(&mut combined.html, child.html, "");
            }
        }
        combined
    }
}

/// Append `next` to `acc`, putting `separator` between them unless `acc`
/// already ends with a line break.
fn append_body(acc: &mut Option<String>, next: Option<String>, separator: &str) {
    let Some(next) = next else { return };
    match acc {
        None => *acc = Some(next),
        Some(existing) => {
            if !existing.ends_with('\n') {
                existing.push_str(separator);
            }
            existing.push_str(&next);
        }
    }
}

/// `in_embedded_message` is true below a `message/rfc822` node: that subtree
/// is another message, so its text parts are never this message's body.
fn extract_payload_recursive(
    part: &Value,
    contents: &mut PayloadContents,
    part_counter: &mut usize,
    in_embedded_message: bool,
) -> Result<NodeBodies, GwsError> {
    let mime_type = part.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");

    let filename = part.get("filename").and_then(|v| v.as_str()).unwrap_or("");

    let body = part.get("body");

    let attachment_id = body
        .and_then(|b| b.get("attachmentId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let body_data = body.and_then(|b| b.get("data")).and_then(|d| d.as_str());

    let body_size = body
        .and_then(|b| b.get("size"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let content_id_header = get_part_header(part, "Content-ID");

    // Gmail gives Content-IDs to regular attachments too (e.g., PDFs), so
    // the disposition decides between inline and attachment below. An
    // `attachment` disposition also makes an unnamed text part a file, not
    // the body.
    let explicitly_attachment = disposition_is_attachment(part);

    // Primary signal: does this part have fetchable binary data?
    // A part is a file (attachment or inline image) when its bytes are fetchable
    // by attachment ID, or when it is named, has a Content-ID or is marked as
    // an attachment, and carries its data inline.
    let part_data = if !attachment_id.is_empty() {
        Some(PartData::AttachmentId(attachment_id.to_string()))
    } else if !filename.is_empty() || content_id_header.is_some() || explicitly_attachment {
        body_data.map(|d| PartData::Inline(d.to_string()))
    } else {
        None
    };
    let is_hydratable = part_data.is_some();

    // A body text part has inline body.data, no attachmentId, no filename,
    // no Content-ID, is not marked as an attachment, and is not inside an
    // encapsulated message.
    let body_text_data = body_data.filter(|_| {
        !is_hydratable
            && !in_embedded_message
            && !explicitly_attachment
            && filename.is_empty()
            && content_id_header.is_none()
    });

    let mut bodies = NodeBodies::default();
    if let Some(data) = body_text_data {
        // MIME types are case-insensitive (RFC 2045 §5.1) and Gmail reports
        // them as the sender wrote them, e.g. `TEXT/PLAIN`.
        let content_type = get_part_header(part, "Content-Type");
        if mime_type.eq_ignore_ascii_case("text/plain") {
            bodies.text = Some(decode_text_body(data, content_type, "text/plain")?);
        } else if mime_type.eq_ignore_ascii_case("text/html") {
            bodies.html = Some(decode_text_body(data, content_type, "text/html")?);
        }
    } else if let Some(part_data) = part_data {
        // This part has fetchable data — classify as inline or attachment
        let index = *part_counter;
        *part_counter += 1;

        // Classify as inline only when Content-ID is present AND
        // Content-Disposition is not explicitly "attachment".
        // Sanitize Content-ID: strip angle brackets and control characters.
        // Content-ID is sender-controlled; CR/LF could inject MIME headers via
        // mail-builder's MessageId, which writes the value raw inside <...>.
        // Treat as absent when the part is explicitly an attachment.
        let content_id = if explicitly_attachment {
            None
        } else {
            content_id_header
                .map(|cid| sanitize_control_chars(strip_angle_brackets(cid)))
                .filter(|cid| !cid.is_empty())
        };

        let resolved_filename = if !filename.is_empty() {
            sanitize_remote_filename(filename, index, mime_type)
        } else {
            synthesize_filename(index, mime_type)
        };

        let sanitized_mime = sanitize_control_chars(mime_type);
        contents.parts.push(OriginalPart {
            filename: resolved_filename,
            content_type: if sanitized_mime.is_empty() {
                "application/octet-stream".to_string()
            } else {
                sanitized_mime
            },
            size: body_size,
            data: part_data,
            content_id,
        });
        // Do NOT recurse into hydratable parts. A message/rfc822 attachment or
        // other encapsulated multipart has its own MIME subtree — recursing would
        // incorrectly pull the attached message's body text and nested parts into
        // the top-level message.
    } else {
        // Only recurse into non-hydratable container nodes (multipart/mixed,
        // etc.). An encapsulated message's files are still collected, but
        // its text is not this message's body.
        let embedded = in_embedded_message || mime_type.eq_ignore_ascii_case("message/rfc822");
        let mut children = Vec::new();
        if let Some(child_parts) = part.get("parts").and_then(|p| p.as_array()) {
            for child in child_parts {
                children.push(extract_payload_recursive(
                    child,
                    contents,
                    part_counter,
                    embedded,
                )?);
            }
        }
        // An encapsulated message is another message: its files are still
        // collected, but its text is not this message's body (joining it
        // would append the forwarded message to this one).
        if !mime_type.eq_ignore_ascii_case("message/rfc822") {
            let alternative = mime_type.eq_ignore_ascii_case("multipart/alternative");
            bodies = NodeBodies::combine(children, alternative);
        }
    }
    Ok(bodies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::base64url;
    use base64::engine::general_purpose::URL_SAFE;

    /// Test-only wrapper: extract the plain text body from a payload using the single-pass walker.
    fn extract_plain_text_body(payload: &Value) -> Option<String> {
        extract_payload_contents(payload).unwrap().body_text
    }

    /// Test-only wrapper: extract the HTML body from a payload using the single-pass walker.
    fn extract_html_body(payload: &Value) -> Option<String> {
        extract_payload_contents(payload).unwrap().body_html
    }

    #[test]
    fn test_original_message_default() {
        let d = OriginalMessage::default();
        assert!(d.thread_id.is_none());
        assert!(d.message_id.is_empty());
        assert!(d.references.is_empty());
        assert!(d.from.email.is_empty());
        assert!(d.from.name.is_none());
        assert!(d.reply_to.is_none());
        assert!(d.to.is_empty());
        assert!(d.cc.is_none());
        assert!(d.subject.is_empty());
        assert!(d.date.is_none());
        assert!(d.body_text.is_empty());
        assert!(d.body_html.is_none());
        assert!(d.parts.is_empty());
    }

    #[test]
    fn test_parse_original_message_minimal() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "fallback text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Subject", "value": "Hi" },
                    { "name": "Message-ID", "value": "<min@example.com>" }
                ],
                "body": {
                    "data": URL_SAFE.encode("Hello")
                }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.thread_id.as_deref(), Some("t1"));
        assert_eq!(original.from.email, "alice@example.com");
        assert_eq!(original.subject, "Hi");
        assert_eq!(original.body_text, "Hello");
        assert_eq!(original.message_id, "min@example.com");
        // Missing optional fields default to None/empty
        assert!(original.reply_to.is_none());
        assert!(original.cc.is_none());
        assert!(original.date.is_none());
        assert!(original.references.is_empty());
        assert!(original.body_html.is_none());
    }

    #[test]
    fn test_parse_original_message_bare_message_id() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Subject", "value": "Hi" },
                    { "name": "Message-ID", "value": "bare-id@example.com" }
                ],
                "body": { "data": URL_SAFE.encode("text") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        // Bare ID (no angle brackets) should be preserved as-is
        assert_eq!(original.message_id, "bare-id@example.com");
    }

    /// RFC 5322 §3.6.4: `References` is `1*msg-id` with *optional* folding
    /// white space between IDs, so `<a@x><b@x>` is two IDs. Splitting on
    /// white space alone glued them into one ID containing `><`.
    #[test]
    fn test_parse_original_message_in_reply_to_starts_references_chain() {
        // A reply whose client wrote In-Reply-To but no References.
        let msg = json!({
            "threadId": "t1",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" },
                    { "name": "In-Reply-To", "value": "<parent@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("text") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert!(original.references.is_empty());
        assert_eq!(original.in_reply_to, vec!["parent@example.com"]);
        assert_eq!(
            crate::helpers::gmail::mime::build_references_chain(&original),
            vec!["parent@example.com", "msg@example.com"]
        );
        // Not part of the +read JSON output.
        assert!(
            serde_json::to_value(&original)
                .unwrap()
                .get("in_reply_to")
                .is_none()
        );
    }

    #[test]
    fn test_parse_original_message_references_without_spaces() {
        let msg = json!({
            "threadId": "t1",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com> (added by relay)" },
                    { "name": "References", "value": "<ref-1@example.com><ref-2@example.com>\r\n <ref-3@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("text") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(
            original.references,
            vec![
                "ref-1@example.com",
                "ref-2@example.com",
                "ref-3@example.com"
            ]
        );
        assert_eq!(original.message_id, "msg@example.com");
    }

    #[test]
    fn test_parse_original_message_missing_payload() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "fallback"
        });
        // Missing payload means no From or Message-ID → error
        let result = parse_original_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_original_message_missing_thread_id() {
        let msg = json!({
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        let result = parse_original_message(&msg).unwrap();
        assert!(result.thread_id.is_none());
    }

    #[test]
    fn test_parse_original_message_missing_from() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        let result = parse_original_message(&msg);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("From"));
    }

    #[test]
    fn test_parse_original_message_missing_message_id() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        // Replies and forwards then go out without In-Reply-To.
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.message_id, "");
        assert_eq!(original.from.email, "alice@example.com");
    }

    #[test]
    fn test_parse_original_message_html_only_renders_text() {
        // When only text/html is present (no text/plain), body_text is the HTML rendered as text
        let msg = json!({
            "threadId": "t1",
            "snippet": "Snippet fallback text",
            "payload": {
                "mimeType": "text/html",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("<p>HTML only</p>") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.body_text.trim(), "HTML only");
        assert_eq!(original.body_html.unwrap(), "<p>HTML only</p>");
    }

    #[test]
    fn test_parse_original_message_snippet_fallback_without_bodies() {
        let msg = json!({
            "snippet": "Snippet only",
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "parts": []
            }
        });
        assert_eq!(
            parse_original_message(&msg).unwrap().body_text,
            "Snippet only"
        );
    }

    fn snippet_only_message(snippet: &str) -> Value {
        json!({
            "snippet": snippet,
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "parts": []
            }
        })
    }

    #[test]
    fn test_snippet_fallback_decodes_html_entities() {
        // Gmail returns `snippet` HTML-escaped; body_text is plain text.
        let msg = snippet_only_message(
            "Tom &amp; Jerry&#39;s &quot;plan&quot;: 1 &lt; 2 &gt; 0, a long line of text \
             that goes well past seventy-eight columns so it must not be wrapped",
        );
        assert_eq!(
            parse_original_message(&msg).unwrap().body_text,
            "Tom & Jerry's \"plan\": 1 < 2 > 0, a long line of text \
             that goes well past seventy-eight columns so it must not be wrapped"
        );
        assert_eq!(
            parse_original_message(&snippet_only_message(""))
                .unwrap()
                .body_text,
            ""
        );
    }

    #[test]
    fn test_snippet_fallback_is_escaped_once_in_html_quote() {
        let original =
            parse_original_message(&snippet_only_message("Tom &amp; Jerry&#39;s")).unwrap();
        assert_eq!(
            super::super::html::resolve_html_body(&original),
            "Tom &amp; Jerry&#39;s"
        );
    }

    #[test]
    fn test_invalid_base64_body_is_an_error() {
        let payload = json!({ "mimeType": "text/plain", "body": { "data": "!!!not base64!!!" } });
        let err = extract_payload_contents(&payload).err().unwrap();
        assert!(err.to_string().contains("invalid base64url"));
    }

    #[test]
    fn test_unpadded_base64_body_is_accepted() {
        // "Hi" encodes to "SGk=" padded; Gmail may omit the padding.
        let payload = json!({ "mimeType": "text/plain", "body": { "data": "SGk" } });
        assert_eq!(extract_plain_text_body(&payload).unwrap(), "Hi");
    }

    /// Gmail returns body bytes in the part's own charset; they must be
    /// decoded with it, not as UTF-8.
    #[test]
    fn test_body_is_decoded_in_its_declared_charset() {
        let latin1 = URL_SAFE.encode([b'c', b'a', b'f', 0xE9]);
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "headers": [{ "name": "content-type", "value": "text/plain; format=flowed; Charset=\"ISO-8859-1\"" }],
                    "body": { "data": latin1 }
                },
                {
                    "mimeType": "text/html",
                    "headers": [{ "name": "Content-Type", "value": "text/html; charset=windows-1252" }],
                    "body": { "data": URL_SAFE.encode([b'<', b'p', b'>', 0x93, b'q', 0x94, b'<', b'/', b'p', b'>']) }
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("caf\u{e9}"));
        assert_eq!(
            contents.body_html.as_deref(),
            Some("<p>\u{201c}q\u{201d}</p>")
        );

        // Shift_JIS for "\u{65e5}\u{672c}" is 93 FA 96 7B.
        let sjis = json!({
            "mimeType": "text/plain",
            "headers": [{ "name": "Content-Type", "value": "text/plain; charset=shift_jis" }],
            "body": { "data": URL_SAFE.encode([0x93, 0xFA, 0x96, 0x7B]) }
        });
        assert_eq!(extract_plain_text_body(&sjis).unwrap(), "\u{65e5}\u{672c}");
    }

    /// A byte-order mark is removed from the body, and a UTF-16 BOM decides
    /// the byte order of a `charset=utf-16` body (RFC 2781 §3.3).
    #[test]
    fn test_body_byte_order_mark_is_honored_and_removed() {
        for (ct, bytes) in [
            (
                "text/plain; charset=utf-8",
                b"\xEF\xBB\xBFHi caf\xC3\xA9".to_vec(),
            ),
            ("text/plain", b"\xEF\xBB\xBFHi caf\xC3\xA9".to_vec()),
            (
                "text/plain; charset=utf-16",
                b"\xFE\xFF\x00H\x00i\x00 \x00c\x00a\x00f\x00\xE9".to_vec(),
            ),
            (
                "text/plain; charset=utf-16",
                b"\xFF\xFEH\x00i\x00 \x00c\x00a\x00f\x00\xE9\x00".to_vec(),
            ),
        ] {
            let payload = json!({
                "mimeType": "text/plain",
                "headers": [{ "name": "Content-Type", "value": ct }],
                "body": { "data": URL_SAFE.encode(&bytes) }
            });
            assert_eq!(
                extract_plain_text_body(&payload).unwrap(),
                "Hi caf\u{e9}",
                "{ct}: {bytes:02X?}"
            );
        }
    }

    #[test]
    fn test_ascii_or_unknown_charset_body_is_decoded_as_utf8() {
        for ct in [
            "text/plain; charset=us-ascii",
            "text/plain; charset=x-unknown-cs",
            "text/plain",
        ] {
            let payload = json!({
                "mimeType": "text/plain",
                "headers": [{ "name": "Content-Type", "value": ct }],
                "body": { "data": URL_SAFE.encode("caf\u{e9}") }
            });
            assert_eq!(
                extract_plain_text_body(&payload).unwrap(),
                "caf\u{e9}",
                "{ct}"
            );
        }
    }

    /// Charsets encoding_rs cannot decode map to its replacement encoding,
    /// which would turn the whole body into one U+FFFD. They are decoded as
    /// UTF-8 instead, so the ASCII text survives.
    #[test]
    fn test_unsupported_charset_body_keeps_its_ascii_text() {
        for (ct, bytes) in [
            (
                "text/plain; charset=ISO-2022-KR",
                b"\x1b$)C\x0e\x30\x21\x0f Meeting at 10:00, see https://example.com/agenda"
                    .as_slice(),
            ),
            (
                "text/plain; charset=hz-gb-2312",
                b"~{<:Ky2;S{#,NpJ)l6HK!#~} Meeting at 10:00, see https://example.com/agenda"
                    .as_slice(),
            ),
        ] {
            let payload = json!({
                "mimeType": "text/plain",
                "headers": [{ "name": "Content-Type", "value": ct }],
                "body": { "data": URL_SAFE.encode(bytes) }
            });
            let text = extract_plain_text_body(&payload).unwrap();
            assert!(
                text.contains("Meeting at 10:00, see https://example.com/agenda"),
                "{ct}: {text:?}"
            );
        }
    }

    #[test]
    fn test_content_type_param() {
        let ct = r#"text/plain; name="a;charset=x.txt"; CHARSET = "utf\-8""#;
        assert_eq!(content_type_param(ct, "charset").as_deref(), Some("utf-8"));
        assert_eq!(content_type_param("text/plain", "charset"), None);
        assert_eq!(content_type_param("text/plain; charset=", "charset"), None);
        assert_eq!(
            content_type_param("text/plain;charset=koi8-r", "charset").as_deref(),
            Some("koi8-r")
        );
    }

    #[test]
    fn test_non_utf8_body_is_decoded_lossily() {
        let data = URL_SAFE.encode([b'c', b'a', b'f', 0xE9]);
        let payload = json!({ "mimeType": "text/plain", "body": { "data": data } });
        assert_eq!(extract_plain_text_body(&payload).unwrap(), "caf\u{FFFD}");
    }

    // --- extract_plain_text_body tests ---

    #[test]
    fn test_extract_plain_text_body_simple() {
        let payload = json!({
            "mimeType": "text/plain",
            "body": {
                "data": URL_SAFE.encode("Hello, world!")
            }
        });
        assert_eq!(extract_plain_text_body(&payload).unwrap(), "Hello, world!");
    }

    #[test]
    fn test_extract_plain_text_body_multipart() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": { "data": URL_SAFE.encode("Plain text body") }
                },
                {
                    "mimeType": "text/html",
                    "body": { "data": URL_SAFE.encode("<p>HTML body</p>") }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Plain text body"
        );
    }

    #[test]
    fn test_extract_plain_text_body_nested_multipart() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": { "data": URL_SAFE.encode("Nested plain text") }
                        },
                        {
                            "mimeType": "text/html",
                            "body": { "data": URL_SAFE.encode("<p>HTML</p>") }
                        }
                    ]
                },
                {
                    "mimeType": "application/pdf",
                    "body": { "attachmentId": "att123" }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Nested plain text"
        );
    }

    /// Apple Mail sends an HTML body with inline attachments as several
    /// `text/html` (and `text/plain`) pieces around the attachments. Every
    /// piece is part of the body, in order.
    #[test]
    fn test_body_split_around_inline_attachments_is_joined() {
        let pdf = |id: &str| {
            json!({
                "mimeType": "application/pdf",
                "filename": format!("{id}.pdf"),
                "headers": [{ "name": "Content-Disposition", "value": "inline" }],
                "body": { "attachmentId": id, "size": 10 }
            })
        };
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "multipart/mixed",
                    "parts": [
                        { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Before\n") } },
                        pdf("A1"),
                        { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("After") } }
                    ]
                },
                {
                    "mimeType": "multipart/mixed",
                    "parts": [
                        { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Before</p>") } },
                        pdf("A2"),
                        { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Middle</p>") } },
                        pdf("A3"),
                        { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>After</p>") } }
                    ]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Before\nAfter"));
        assert_eq!(
            contents.body_html.as_deref(),
            Some("<p>Before</p><p>Middle</p><p>After</p>")
        );
        assert_eq!(contents.parts.len(), 3);
    }

    /// Plain-text pieces that don't end in a line break are joined with one,
    /// so the words of adjacent pieces don't run together.
    #[test]
    fn test_split_plain_body_pieces_are_separated_by_a_line_break() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Body") } },
                { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("-- \nList footer") } }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).as_deref(),
            Some("Body\n-- \nList footer")
        );
    }

    /// The alternatives of `multipart/alternative` are the same content, so
    /// they are never joined: the first of each kind wins.
    #[test]
    fn test_alternative_bodies_are_not_joined() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Plain") } },
                { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Rich</p>") } },
                { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Richer</p>") } }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Plain"));
        assert_eq!(contents.body_html.as_deref(), Some("<p>Rich</p>"));
    }

    /// MIME types are case-insensitive: a body part Gmail reports as
    /// `TEXT/PLAIN` or `Text/Html` is still the body.
    #[test]
    fn test_body_mime_types_match_case_insensitively() {
        let payload = json!({
            "mimeType": "Multipart/Alternative",
            "parts": [
                {
                    "mimeType": "TEXT/PLAIN",
                    "body": { "data": URL_SAFE.encode("Plain body") }
                },
                {
                    "mimeType": "Text/Html",
                    "body": { "data": URL_SAFE.encode("<p>HTML body</p>") }
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Plain body"));
        assert_eq!(contents.body_html.as_deref(), Some("<p>HTML body</p>"));
        assert!(contents.parts.is_empty());
    }

    /// Joining must not pull in the text of an embedded message or of a
    /// text part marked as an attachment.
    #[test]
    fn test_join_skips_embedded_messages_and_attachment_text() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Outer") } },
                {
                    "mimeType": "text/plain",
                    "headers": [{ "name": "Content-Disposition", "value": "attachment" }],
                    "body": { "data": URL_SAFE.encode("log line") }
                },
                {
                    "mimeType": "message/rfc822",
                    "body": { "size": 0 },
                    "parts": [{
                        "mimeType": "multipart/mixed",
                        "parts": [
                            { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Inner") } },
                            {
                                "mimeType": "application/pdf",
                                "filename": "inner.pdf",
                                "body": { "attachmentId": "INNER", "size": 10 }
                            }
                        ]
                    }]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Outer"));
        assert_eq!(contents.body_html, None);
        // The text part marked as an attachment is a file (#328), not body.
        assert_eq!(contents.parts.len(), 2);
        assert_eq!(contents.parts[0].attachment_id(), None);
        assert_eq!(contents.parts[1].attachment_id(), Some("INNER"));
    }

    /// A large body that Gmail returns by attachment ID is read from the
    /// fetched bytes, and is not listed as a file.
    #[test]
    fn test_body_returned_by_reference_is_inlined_and_read_as_the_body() {
        let mut payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        { "mimeType": "text/plain", "body": { "attachmentId": "BODY_T", "size": 900000 } },
                        { "mimeType": "text/html", "body": { "attachmentId": "BODY_H", "size": 900000 } }
                    ]
                },
                {
                    "mimeType": "text/plain",
                    "headers": [{ "name": "Content-Disposition", "value": "attachment" }],
                    "body": { "attachmentId": "LOG", "size": 10 }
                },
                {
                    "mimeType": "message/rfc822",
                    "parts": [
                        { "mimeType": "text/plain", "body": { "attachmentId": "INNER", "size": 10 } }
                    ]
                },
                {
                    "mimeType": "text/html",
                    "filename": "page.html",
                    "body": { "attachmentId": "FILE", "size": 10 }
                }
            ]
        });
        assert_eq!(
            referenced_body_attachment_ids(&payload),
            vec!["BODY_T", "BODY_H"]
        );

        let fetched = std::collections::HashMap::from([
            ("BODY_T".to_string(), b"Long plain body".to_vec()),
            ("BODY_H".to_string(), b"<p>Long HTML body</p>".to_vec()),
        ]);
        inline_referenced_bodies(&mut payload, &fetched).unwrap();
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Long plain body"));
        assert_eq!(contents.body_html.as_deref(), Some("<p>Long HTML body</p>"));
        let ids: Vec<_> = contents.parts.iter().map(|p| p.attachment_id()).collect();
        // The embedded message's text is not fetched as this message's body;
        // it stays a downloadable part, like the embedded message's other files.
        assert_eq!(ids, vec![Some("LOG"), Some("INNER"), Some("FILE")]);
    }

    /// A text part marked as an attachment is a file, not a body Gmail
    /// returned by reference, however the disposition is spaced or cased:
    /// the body walk must agree with the file listing, or the attachment's
    /// bytes are fetched and read as the message body.
    #[test]
    fn test_spaced_attachment_disposition_is_not_fetched_as_the_body() {
        for value in [
            " attachment",
            "\tATTACHMENT; filename=\"log.txt\"",
            "Attachment",
        ] {
            let mut payload = json!({
                "mimeType": "multipart/mixed",
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": "SGk", "size": 2 } },
                    {
                        "mimeType": "text/plain",
                        "headers": [{ "name": "Content-Disposition", "value": value }],
                        "body": { "attachmentId": "LOG", "size": 10 }
                    }
                ]
            });
            assert!(
                referenced_body_attachment_ids(&payload).is_empty(),
                "{value:?}"
            );
            inline_referenced_bodies(&mut payload, &Default::default()).unwrap();
            let contents = extract_payload_contents(&payload).unwrap();
            assert_eq!(contents.body_text.as_deref(), Some("Hi"), "{value:?}");
            let ids: Vec<_> = contents.parts.iter().map(|p| p.attachment_id()).collect();
            assert_eq!(ids, vec![Some("LOG")], "{value:?}");
        }
        // Another disposition is still a body returned by reference.
        let part = json!({
            "mimeType": "text/plain",
            "headers": [{ "name": "Content-Disposition", "value": " inline" }],
            "body": { "attachmentId": "BODY", "size": 10 }
        });
        assert_eq!(referenced_body_attachment_ids(&part), vec!["BODY"]);
    }

    #[test]
    fn test_inlining_an_unfetched_body_part_is_an_error() {
        let mut payload = json!({
            "mimeType": "text/plain",
            "body": { "attachmentId": "BODY", "size": 900000 }
        });
        let err = inline_referenced_bodies(&mut payload, &Default::default()).unwrap_err();
        assert!(err.to_string().contains("BODY"), "{err}");
    }

    #[test]
    fn test_extract_plain_text_body_no_text_part() {
        let payload = json!({
            "mimeType": "text/html",
            "body": { "data": URL_SAFE.encode("<p>Only HTML</p>") }
        });
        assert!(extract_plain_text_body(&payload).is_none());
    }

    #[test]
    fn test_append_address_list_header_value() {
        let mut header_value = String::new();

        append_address_list_header_value(&mut header_value, "alice@example.com");
        append_address_list_header_value(&mut header_value, "bob@example.com");
        append_address_list_header_value(&mut header_value, "");

        assert_eq!(header_value, "alice@example.com, bob@example.com");
    }

    #[test]
    fn test_parse_original_message_header_names_are_case_insensitive() {
        // RFC 5322 header field names are case-insensitive; senders emit `CC`,
        // `Reply-to`, `Message-id` and so on.
        let msg = json!({
            "threadId": "thread-case",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "FROM", "value": "alice@example.com" },
                    { "name": "Reply-to", "value": "team@example.com" },
                    { "name": "to", "value": "bob@example.com" },
                    { "name": "CC", "value": "dave@example.com" },
                    { "name": "cc", "value": "erin@example.com" },
                    { "name": "subject", "value": "Hello" },
                    { "name": "DATE", "value": "Fri, 6 Mar 2026 12:00:00 +0000" },
                    { "name": "Message-id", "value": "<msg@example.com>" },
                    { "name": "REFERENCES", "value": "<ref-1@example.com>" },
                    { "name": "X-Cc", "value": "notcc@example.com" }
                ],
                "body": { "data": URL_SAFE.encode("Body") }
            }
        });

        let original = parse_original_message(&msg).unwrap();

        assert_eq!(original.from.email, "alice@example.com");
        let reply_to = original.reply_to.unwrap();
        assert_eq!(reply_to.len(), 1);
        assert_eq!(reply_to[0].email, "team@example.com");
        assert_eq!(original.to.len(), 1);
        assert_eq!(original.to[0].email, "bob@example.com");
        let cc = original.cc.unwrap();
        assert_eq!(cc.len(), 2);
        assert_eq!(cc[0].email, "dave@example.com");
        assert_eq!(cc[1].email, "erin@example.com");
        assert_eq!(original.subject, "Hello");
        assert_eq!(
            original.date.as_deref(),
            Some("Fri, 6 Mar 2026 12:00:00 +0000")
        );
        assert_eq!(original.message_id, "msg@example.com");
        assert_eq!(original.references, vec!["ref-1@example.com"]);
    }

    #[test]
    fn test_parse_original_message_rejects_lookalike_header_names() {
        // Case-insensitive matching must not turn a different header into a
        // required one: `X-From` / `Message-ID-Old` are not `From` / `Message-ID`.
        let msg = json!({
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "X-From", "value": "alice@example.com" },
                    { "name": "message-id", "value": "<msg@example.com>" }
                ]
            }
        });
        let err = parse_original_message(&msg).err().unwrap();
        assert!(err.to_string().contains("missing From header"), "{err}");

        let msg = json!({
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "from", "value": "alice@example.com" },
                    { "name": "Message-ID-Old", "value": "<msg@example.com>" }
                ]
            }
        });
        // No Message-ID header (only a look-alike name): parsed, with no ID.
        assert_eq!(parse_original_message(&msg).unwrap().message_id, "");
    }

    #[test]
    fn reply_to_naming_no_mailbox_is_treated_as_absent() {
        for reply_to in ["undisclosed-recipients:;", "Team: ;", "<>"] {
            let msg = json!({
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [
                        { "name": "From", "value": "alice@example.com" },
                        { "name": "Reply-To", "value": reply_to },
                        { "name": "Message-ID", "value": "<msg@example.com>" }
                    ],
                    "body": { "data": URL_SAFE.encode("Hello") }
                }
            });
            let original = parse_original_message(&msg).unwrap();
            assert!(original.reply_to.is_none(), "{reply_to:?}");
        }
    }

    #[test]
    fn test_parse_original_message_concatenates_repeated_address_and_reference_headers() {
        let msg = json!({
            "threadId": "thread-123",
            "snippet": "Snippet fallback",
            "payload": {
                "mimeType": "text/html",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Reply-To", "value": "team@example.com" },
                    { "name": "Reply-To", "value": "owner@example.com" },
                    { "name": "To", "value": "bob@example.com" },
                    { "name": "To", "value": "carol@example.com" },
                    { "name": "Cc", "value": "dave@example.com" },
                    { "name": "Cc", "value": "erin@example.com" },
                    { "name": "Subject", "value": "Hello" },
                    { "name": "Date", "value": "Fri, 6 Mar 2026 12:00:00 +0000" },
                    { "name": "Message-ID", "value": "<msg@example.com>" },
                    { "name": "References", "value": "<ref-1@example.com>" },
                    { "name": "References", "value": "<ref-2@example.com>" }
                ],
                "body": {
                    "data": URL_SAFE.encode("<p>HTML only</p>")
                }
            }
        });

        let original = parse_original_message(&msg).unwrap();

        assert_eq!(original.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(original.from.email, "alice@example.com");
        let reply_to = original.reply_to.unwrap();
        assert_eq!(reply_to.len(), 2);
        assert_eq!(reply_to[0].email, "team@example.com");
        assert_eq!(reply_to[1].email, "owner@example.com");
        assert_eq!(original.to.len(), 2);
        assert_eq!(original.to[0].email, "bob@example.com");
        assert_eq!(original.to[1].email, "carol@example.com");
        let cc = original.cc.unwrap();
        assert_eq!(cc.len(), 2);
        assert_eq!(cc[0].email, "dave@example.com");
        assert_eq!(cc[1].email, "erin@example.com");
        assert_eq!(original.subject, "Hello");
        assert_eq!(
            original.date.as_deref(),
            Some("Fri, 6 Mar 2026 12:00:00 +0000")
        );
        assert_eq!(original.message_id, "msg@example.com");
        assert_eq!(
            original.references,
            vec!["ref-1@example.com", "ref-2@example.com"]
        );
        assert_eq!(original.body_text.trim(), "HTML only");
        assert_eq!(original.body_html.as_deref(), Some("<p>HTML only</p>"));
    }

    #[test]
    fn test_parse_original_message_multipart_alternative() {
        let msg = json!({
            "threadId": "thread-456",
            "snippet": "Snippet ignored when text/plain exists",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "To", "value": "bob@example.com" },
                    { "name": "Subject", "value": "Hello" },
                    { "name": "Date", "value": "Fri, 6 Mar 2026 12:00:00 +0000" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": URL_SAFE.encode("Plain text body") }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": URL_SAFE.encode("<p>Rich HTML body</p>") }
                    }
                ]
            }
        });

        let original = parse_original_message(&msg).unwrap();

        assert_eq!(original.body_text, "Plain text body");
        assert_eq!(original.body_html.as_deref(), Some("<p>Rich HTML body</p>"));
    }

    #[test]
    fn test_parse_original_message_blank_text_part_uses_html() {
        // Upstream googleworkspace/cli#889: a whitespace-only text/plain
        // alternative must not hide the real (HTML) body.
        let msg = json!({
            "threadId": "t1",
            "snippet": "Snippet",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": URL_SAFE.encode("\r\n \r\n") }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": URL_SAFE.encode(
                            "<p>Reset: <a href=\"https://example.com/r?t=1\">link</a></p>"
                        ) }
                    }
                ]
            }
        });

        let original = parse_original_message(&msg).unwrap();

        assert!(
            original.body_text.contains("https://example.com/r?t=1"),
            "body_text should be the rendered HTML, got {:?}",
            original.body_text
        );
    }

    #[test]
    fn test_parse_original_message_blank_text_part_without_html_is_kept() {
        // With no HTML alternative, a blank text part is the body: it is not
        // replaced by the (truncated) snippet.
        let msg = json!({
            "snippet": "Snippet",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("\r\n") }
            }
        });
        assert_eq!(parse_original_message(&msg).unwrap().body_text, "\r\n");
    }

    #[test]
    fn test_extract_html_body_direct() {
        let payload = json!({
            "mimeType": "text/html",
            "body": {
                "data": URL_SAFE.encode("<p>Hello</p>")
            }
        });
        assert_eq!(extract_html_body(&payload).as_deref(), Some("<p>Hello</p>"));
    }

    #[test]
    fn test_extract_html_body_from_multipart() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": { "data": URL_SAFE.encode("plain text") }
                },
                {
                    "mimeType": "text/html",
                    "body": { "data": URL_SAFE.encode("<p>rich text</p>") }
                }
            ]
        });
        assert_eq!(
            extract_html_body(&payload).as_deref(),
            Some("<p>rich text</p>")
        );
    }

    #[test]
    fn test_extract_html_body_missing() {
        let payload = json!({
            "mimeType": "text/plain",
            "body": { "data": URL_SAFE.encode("only plain") }
        });
        assert!(extract_html_body(&payload).is_none());
    }

    #[test]
    fn test_extract_html_body_from_nested_multipart() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": { "data": URL_SAFE.encode("plain text") }
                        },
                        {
                            "mimeType": "text/html",
                            "body": { "data": URL_SAFE.encode("<p>Nested HTML</p>") }
                        }
                    ]
                },
                {
                    "mimeType": "application/pdf",
                    "body": { "attachmentId": "att123" }
                }
            ]
        });
        assert_eq!(
            extract_html_body(&payload).as_deref(),
            Some("<p>Nested HTML</p>")
        );
    }

    #[test]
    fn test_extract_payload_contents_simple() {
        let text_data = base64url("Hello plain text");
        let html_data = base64url("<p>Hello HTML</p>");
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": text_data, "size": 16 } },
                { "mimeType": "text/html", "body": { "data": html_data, "size": 18 } },
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Hello plain text"));
        assert_eq!(contents.body_html.as_deref(), Some("<p>Hello HTML</p>"));
        assert!(contents.parts.is_empty());
    }

    #[test]
    fn test_extract_payload_contents_with_attachment() {
        let text_data = base64url("Body text");
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": text_data, "size": 9 } },
                {
                    "mimeType": "application/pdf",
                    "filename": "report.pdf",
                    "body": { "attachmentId": "ATT123", "size": 1024 },
                    "headers": [
                        { "name": "Content-Disposition", "value": "attachment; filename=\"report.pdf\"" }
                    ]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Body text"));
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "report.pdf");
        assert_eq!(contents.parts[0].content_type, "application/pdf");
        assert_eq!(contents.parts[0].attachment_id(), Some("ATT123"));
        assert_eq!(contents.parts[0].size, 1024);
        assert!(!contents.parts[0].is_inline());
        assert!(contents.parts[0].content_id.is_none());
    }

    #[test]
    fn test_extract_payload_contents_with_inline_image() {
        let text_data = base64url("Body");
        let html_data = base64url("<p>See <img src=\"cid:img1@example.com\"></p>");
        let payload = json!({
            "mimeType": "multipart/related",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        { "mimeType": "text/plain", "body": { "data": text_data, "size": 4 } },
                        { "mimeType": "text/html", "body": { "data": html_data, "size": 40 } },
                    ]
                },
                {
                    "mimeType": "image/png",
                    "filename": "photo.png",
                    "body": { "attachmentId": "INLINE1", "size": 5000 },
                    "headers": [
                        { "name": "Content-ID", "value": "<img1@example.com>" },
                        { "name": "Content-Disposition", "value": "inline; filename=\"photo.png\"" }
                    ]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        assert!(contents.parts[0].is_inline());
        assert_eq!(
            contents.parts[0].content_id.as_deref(),
            Some("img1@example.com")
        );
        assert_eq!(contents.parts[0].filename, "photo.png");
    }

    #[test]
    fn test_extract_payload_contents_no_filename_synthesis() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": base64url("hi"), "size": 2 } },
                {
                    "mimeType": "image/jpeg",
                    "filename": "",
                    "body": { "attachmentId": "ATT_NO_NAME", "size": 500 },
                    "headers": []
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "part-0.jpg");
        assert!(!contents.parts[0].is_inline());
    }

    #[test]
    fn test_content_id_normalization() {
        let payload = json!({
            "mimeType": "image/png",
            "filename": "logo.png",
            "body": { "attachmentId": "CID_TEST", "size": 100 },
            "headers": [
                { "name": "Content-ID", "value": "<logo@company.com>" }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        // Angle brackets should be stripped
        assert_eq!(
            contents.parts[0].content_id.as_deref(),
            Some("logo@company.com")
        );
    }

    #[test]
    fn test_content_id_crlf_injection_sanitized() {
        // Content-ID is sender-controlled; CR/LF could inject MIME headers.
        // Verify that control characters are stripped.
        let payload = json!({
            "mimeType": "image/png",
            "filename": "evil.png",
            "body": { "attachmentId": "INJECT_TEST", "size": 100 },
            "headers": [
                { "name": "Content-ID", "value": "<img1@example.com\r\nX-Injected: yes>" }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        // CR/LF stripped, part is still inline
        assert!(contents.parts[0].is_inline());
        let cid = contents.parts[0].content_id.as_deref().unwrap();
        assert!(!cid.contains('\r'));
        assert!(!cid.contains('\n'));
        assert_eq!(cid, "img1@example.comX-Injected: yes");
    }

    #[test]
    fn test_content_id_all_control_chars_becomes_none() {
        // A Content-ID that is entirely control characters should be treated as absent,
        // making the part a regular attachment instead of inline.
        let payload = json!({
            "mimeType": "image/png",
            "filename": "weird.png",
            "body": { "attachmentId": "EMPTY_CID", "size": 100 },
            "headers": [
                { "name": "Content-ID", "value": "<\r\n>" }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        assert!(!contents.parts[0].is_inline());
        assert!(contents.parts[0].content_id.is_none());
    }

    #[test]
    fn test_attachment_with_content_id_and_disposition_attachment_is_not_inline() {
        // Gmail gives Content-IDs to regular attachments (e.g., PDFs). A part
        // with Content-Disposition: attachment should be classified as a regular
        // attachment regardless of Content-ID presence.
        let payload = json!({
            "mimeType": "application/pdf",
            "filename": "report.pdf",
            "body": { "attachmentId": "PDF1", "size": 50000 },
            "headers": [
                { "name": "Content-Disposition", "value": "attachment; filename=\"report.pdf\"" },
                { "name": "Content-ID", "value": "<some-cid@example.com>" }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        // Should be classified as regular attachment, NOT inline
        assert!(!contents.parts[0].is_inline());
        assert!(contents.parts[0].content_id.is_none());
    }

    #[test]
    fn test_extract_payload_contents_does_not_recurse_into_attachments() {
        // A message/rfc822 attachment has its own MIME subtree. The walker
        // should NOT recurse into it — the attached message's body and parts
        // should not leak into the top-level message.
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": { "data": base64url("Outer body"), "size": 10 }
                },
                {
                    "mimeType": "message/rfc822",
                    "filename": "attached.eml",
                    "body": { "attachmentId": "EML1", "size": 5000 },
                    "headers": [],
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": { "data": base64url("Inner body — should NOT be extracted"), "size": 40 }
                        },
                        {
                            "mimeType": "application/pdf",
                            "filename": "inner.pdf",
                            "body": { "attachmentId": "INNER_ATT", "size": 1000 },
                            "headers": []
                        }
                    ]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        // Should extract the outer body text
        assert_eq!(contents.body_text.as_deref(), Some("Outer body"));
        // Should have exactly one part: the message/rfc822 attachment
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "attached.eml");
        assert_eq!(contents.parts[0].attachment_id(), Some("EML1"));
        // The inner body and inner attachment should NOT appear
        assert_ne!(
            contents.body_text.as_deref(),
            Some("Inner body \u{2014} should NOT be extracted")
        );
    }

    #[test]
    fn test_header_case_insensitive() {
        let payload = json!({
            "mimeType": "image/gif",
            "filename": "spacer.gif",
            "body": { "attachmentId": "CASE_TEST", "size": 43 },
            "headers": [
                { "name": "content-id", "value": "<spacer@example.com>" },
                { "name": "content-disposition", "value": "inline" }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        assert!(contents.parts[0].is_inline());
        assert_eq!(
            contents.parts[0].content_id.as_deref(),
            Some("spacer@example.com")
        );
    }

    #[test]
    fn test_filename_control_char_sanitization() {
        let payload = json!({
            "mimeType": "application/pdf",
            "filename": "report\x00\x0d.pdf",
            "body": { "attachmentId": "SANITIZE_TEST", "size": 100 },
            "headers": []
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "report.pdf");
    }

    #[test]
    fn test_parse_original_message_populates_parts() {
        let msg = json!({
            "threadId": "thread1",
            "snippet": "fallback",
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "To", "value": "bob@example.com" },
                    { "name": "Subject", "value": "Files" },
                    { "name": "Message-ID", "value": "<msg1@example.com>" },
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": base64url("Hello"), "size": 5 }
                    },
                    {
                        "mimeType": "application/pdf",
                        "filename": "report.pdf",
                        "body": { "attachmentId": "ATT1", "size": 2048 },
                        "headers": []
                    },
                    {
                        "mimeType": "image/png",
                        "filename": "photo.png",
                        "body": { "attachmentId": "ATT2", "size": 4096 },
                        "headers": [
                            { "name": "Content-ID", "value": "<img1@example.com>" }
                        ]
                    }
                ]
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.body_text, "Hello");
        assert_eq!(original.parts.len(), 2);
        // First part: regular attachment
        assert_eq!(original.parts[0].filename, "report.pdf");
        assert!(!original.parts[0].is_inline());
        assert_eq!(original.parts[0].attachment_id(), Some("ATT1"));
        // Second part: inline image
        assert_eq!(original.parts[1].filename, "photo.png");
        assert!(original.parts[1].is_inline());
        assert_eq!(
            original.parts[1].content_id.as_deref(),
            Some("img1@example.com")
        );
    }

    #[test]
    fn test_synthesize_filename_jpeg() {
        assert_eq!(synthesize_filename(0, "image/jpeg"), "part-0.jpg");
    }

    #[test]
    fn test_synthesize_filename_svg() {
        assert_eq!(synthesize_filename(1, "image/svg+xml"), "part-1.svg");
    }

    #[test]
    fn test_synthesize_filename_octet_stream() {
        assert_eq!(
            synthesize_filename(2, "application/octet-stream"),
            "part-2.bin"
        );
    }

    #[test]
    fn test_synthesize_filename_no_slash() {
        assert_eq!(synthesize_filename(0, "weirdtype"), "part-0.bin");
    }

    // --- sanitize_remote_filename edge cases ---

    #[test]
    fn test_sanitize_remote_filename_all_control_chars() {
        // All control characters → falls back to synthesized name
        assert_eq!(
            sanitize_remote_filename("\x00\x01\x02", 0, "application/pdf"),
            "part-0.pdf"
        );
    }

    #[test]
    fn test_sanitize_remote_filename_whitespace_only() {
        assert_eq!(
            sanitize_remote_filename("   ", 0, "image/png"),
            "part-0.png"
        );
    }

    /// An unnamed text part marked `Content-Disposition: attachment` is a
    /// file, not the body: it must not replace the HTML body nor vanish
    /// from the attachment list.
    #[test]
    fn test_unnamed_attachment_disposition_text_part_is_an_attachment() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Real body</p>") } },
                {
                    "mimeType": "text/plain",
                    "headers": [{ "name": "Content-Disposition", "value": "attachment" }],
                    "body": { "data": URL_SAFE.encode("log line"), "size": 8 }
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text, None);
        assert_eq!(contents.body_html.as_deref(), Some("<p>Real body</p>"));
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "part-0.txt");
        assert_eq!(
            contents.parts[0].data,
            PartData::Inline(URL_SAFE.encode("log line"))
        );
    }

    /// A message/rfc822 node Gmail does not expose as one file still holds
    /// another message: its text must not become the outer message's body.
    #[test]
    fn test_embedded_message_text_is_not_the_body() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Outer</p>") } },
                {
                    "mimeType": "message/rfc822",
                    "body": { "size": 0 },
                    "parts": [{
                        "mimeType": "multipart/mixed",
                        "parts": [
                            { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Inner") } },
                            { "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Inner</p>") } },
                            {
                                "mimeType": "application/pdf",
                                "filename": "inner.pdf",
                                "body": { "attachmentId": "INNER", "size": 10 }
                            }
                        ]
                    }]
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text, None);
        assert_eq!(contents.body_html.as_deref(), Some("<p>Outer</p>"));
        // The embedded message's file stays downloadable.
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].attachment_id(), Some("INNER"));
    }

    #[test]
    fn test_named_part_with_inline_data_is_an_attachment() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Body") } },
                {
                    "mimeType": "text/csv",
                    "filename": "small.csv",
                    "body": { "data": URL_SAFE.encode("a,b"), "size": 3 }
                }
            ]
        });
        let contents = extract_payload_contents(&payload).unwrap();
        assert_eq!(contents.body_text.as_deref(), Some("Body"));
        assert_eq!(contents.parts.len(), 1);
        assert_eq!(contents.parts[0].filename, "small.csv");
        assert_eq!(
            contents.parts[0].data,
            PartData::Inline(URL_SAFE.encode("a,b"))
        );
    }
}
