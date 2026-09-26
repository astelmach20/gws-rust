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
    /// Bare message ID (no angle brackets), e.g. `"abc@example.com"`.
    pub message_id: String,
    /// Bare message IDs (no angle brackets) forming the references chain.
    pub references: Vec<String>,
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

    let message_id = parse_msg_ids(&parsed_headers.message_id)
        .into_iter()
        .next()
        .ok_or_else(|| GwsError::other("Message is missing Message-ID header"))?;

    let PayloadContents {
        body_text: extracted_text,
        body_html,
        parts: original_parts,
    } = match msg.get("payload") {
        Some(payload) => extract_payload_contents(payload)?,
        None => PayloadContents::default(),
    };

    // Prefer the text/plain part; otherwise render the HTML part as text;
    // only when the message has neither fall back to the API snippet. A
    // whitespace-only text/plain alternative is a stub, not the body, so it
    // yields to a non-blank HTML part.
    let html_is_blank = body_html.as_deref().is_none_or(|h| h.trim().is_empty());
    let body_text = match extracted_text {
        Some(text) if html_is_blank || !text.trim().is_empty() => text,
        _ => match &body_html {
            Some(html) => html_to_text(html)?,
            None => {
                tracing::warn!(
                    "message has no text/plain or text/html body part; using the API snippet, which may be truncated"
                );
                snippet
            }
        },
    };

    let references = parse_msg_ids(&parsed_headers.references);

    let reply_to = non_empty_then(&parsed_headers.reply_to, Mailbox::parse_header_list);
    let cc = non_empty_then(&parsed_headers.cc, Mailbox::parse_header_list);
    let date = Some(parsed_headers.date).filter(|s| !s.is_empty());

    Ok(OriginalMessage {
        thread_id,
        message_id,
        references,
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

/// Decode a base64url-encoded text body part.
///
/// Invalid base64 violates the API contract and is an error. Bodies that are
/// not valid UTF-8 (legacy charsets) are decoded lossily with an explicit
/// warning, since the text is still useful for quoting and reading.
fn decode_text_body(data: &str, mime_label: &str) -> Result<String, GwsError> {
    let decoded = decode_base64url(data).map_err(|e| {
        GwsError::other(format!("{mime_label} body has invalid base64url data: {e}"))
    })?;
    match String::from_utf8(decoded) {
        Ok(s) => Ok(s),
        Err(e) => {
            tracing::warn!(
                "{mime_label} body is not valid UTF-8; invalid bytes were replaced with U+FFFD"
            );
            Ok(String::from_utf8_lossy(e.as_bytes()).into_owned())
        }
    }
}

/// Synthesize a filename from the part index and MIME type when no filename is present.
/// e.g., `"image/png"` at index 1 → `"part-1.png"`.
fn synthesize_filename(part_index: usize, mime_type: &str) -> String {
    let ext = mime_type
        .split('/')
        .nth(1)
        .map(|sub| match sub {
            "jpeg" => "jpg",
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

/// Walk the MIME payload tree in a single pass, collecting the text body, HTML body,
/// and metadata for all attachment/inline parts.
fn extract_payload_contents(payload: &Value) -> Result<PayloadContents, GwsError> {
    let mut contents = PayloadContents::default();
    extract_payload_recursive(payload, &mut contents, &mut 0)?;
    Ok(contents)
}

fn extract_payload_recursive(
    part: &Value,
    contents: &mut PayloadContents,
    part_counter: &mut usize,
) -> Result<(), GwsError> {
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

    // Primary signal: does this part have fetchable binary data?
    // A part is a file (attachment or inline image) when its bytes are fetchable
    // by attachment ID, or when it is named and carries its data inline.
    let part_data = if !attachment_id.is_empty() {
        Some(PartData::AttachmentId(attachment_id.to_string()))
    } else if !filename.is_empty() || content_id_header.is_some() {
        body_data.map(|d| PartData::Inline(d.to_string()))
    } else {
        None
    };
    let is_hydratable = part_data.is_some();

    // A body text part has inline body.data, no attachmentId, no filename, and no Content-ID.
    let body_text_data =
        body_data.filter(|_| !is_hydratable && filename.is_empty() && content_id_header.is_none());

    if let Some(data) = body_text_data {
        if mime_type == "text/plain" && contents.body_text.is_none() {
            contents.body_text = Some(decode_text_body(data, "text/plain")?);
        } else if mime_type == "text/html" && contents.body_html.is_none() {
            contents.body_html = Some(decode_text_body(data, "text/html")?);
        }
    } else if let Some(part_data) = part_data {
        // This part has fetchable data — classify as inline or attachment
        let index = *part_counter;
        *part_counter += 1;

        // Classify as inline only when Content-ID is present AND
        // Content-Disposition is not explicitly "attachment". Gmail gives
        // Content-IDs to regular attachments too (e.g., PDFs), so Content-ID
        // alone is not sufficient — we must check disposition.
        let disposition_header = get_part_header(part, "Content-Disposition");
        let explicitly_attachment = disposition_header
            .map(|d| d.to_ascii_lowercase().starts_with("attachment"))
            .unwrap_or(false);

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
        // Only recurse into non-hydratable container nodes (multipart/mixed, etc.)
        if let Some(child_parts) = part.get("parts").and_then(|p| p.as_array()) {
            for child in child_parts {
                extract_payload_recursive(child, contents, part_counter)?;
            }
        }
    }
    Ok(())
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
        let result = parse_original_message(&msg);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Message-ID"));
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
        let err = parse_original_message(&msg).err().unwrap();
        assert!(
            err.to_string().contains("missing Message-ID header"),
            "{err}"
        );
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
