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

//! Construction of outgoing RFC 5322 / MIME messages.

use super::prelude::*;

/// Threading headers for reply/forward.
///
/// IDs must be bare (no angle brackets) — `set_threading_headers` passes them to
/// mail-builder which adds angle brackets per RFC 5322. `in_reply_to` is a single
/// message ID (the direct parent); `references` is the full ordered chain.
/// The references chain should be fully assembled via `build_references_chain`
/// before constructing this.
pub(super) struct ThreadingHeaders<'a> {
    pub in_reply_to: &'a str,
    pub references: &'a [String],
}

/// Build the full references chain for threading: existing references + current message ID.
pub(super) fn build_references_chain(original: &OriginalMessage) -> Vec<String> {
    let mut refs = original.references.clone();
    if !original.message_id.is_empty() {
        refs.push(original.message_id.clone());
    }
    refs
}

/// Set threading headers on a `mail_builder::MessageBuilder`.
/// See `ThreadingHeaders` for the bare-ID convention.
///
/// Every ID is checked before it is written: mail-builder wraps it in angle
/// brackets verbatim, so an ID containing `<`, `>`, white space or a control
/// character would produce a malformed `In-Reply-To`/`References` header.
pub(super) fn set_threading_headers<'x>(
    mb: mail_builder::MessageBuilder<'x>,
    threading: &ThreadingHeaders<'x>,
) -> Result<mail_builder::MessageBuilder<'x>, GwsError> {
    validate_bare_msg_id(threading.in_reply_to)?;
    for id in threading.references {
        validate_bare_msg_id(id)?;
    }

    use mail_builder::headers::message_id::MessageId;

    let in_reply_to = MessageId::new(threading.in_reply_to);
    let refs = MessageId {
        id: threading
            .references
            .iter()
            .map(|id| id.as_str().into())
            .collect(),
    };

    Ok(mb.in_reply_to(in_reply_to).references(refs))
}

/// Reject a message ID that cannot be written as `<id>` in a threading header.
fn validate_bare_msg_id(id: &str) -> Result<(), GwsError> {
    let bad = id.is_empty()
        || id
            .chars()
            .any(|c| c == '<' || c == '>' || c.is_whitespace() || c.is_control());
    if bad {
        return Err(GwsError::Validation(format!(
            "cannot thread the message: message ID {:?} is empty or contains angle brackets, \
             white space or control characters",
            sanitize_for_terminal(id)
        )));
    }
    Ok(())
}

/// Apply optional From, CC, and BCC headers to a `MessageBuilder`.
pub(super) fn apply_optional_headers<'x>(
    mut mb: mail_builder::MessageBuilder<'x>,
    from: Option<&'x [Mailbox]>,
    cc: Option<&'x [Mailbox]>,
    bcc: Option<&'x [Mailbox]>,
) -> mail_builder::MessageBuilder<'x> {
    if let Some(from) = from {
        mb = mb.from(to_mb_address_list(from));
    }
    if let Some(cc) = cc {
        mb = mb.cc(to_mb_address_list(cc));
    }
    if let Some(bcc) = bcc {
        mb = mb.bcc(to_mb_address_list(bcc));
    }
    mb
}

/// Set the body, add any attachments, and write the finished message to a string.
///
/// MIME structure:
/// * Plain text: `text/plain`, wrapped in `multipart/mixed` when there are attachments.
/// * HTML: `multipart/alternative` with a generated `text/plain` rendering and
///   the `text/html` part, so text-only clients get a readable body. When the
///   message has inline images (parts with a `content_id`), the HTML side is a
///   `multipart/related` container so `cid:` references render. Gmail rewrites
///   `Content-Disposition: inline` to `attachment` for parts in
///   `multipart/mixed`, so the explicit `multipart/related` is required.
///
/// In plain-text mode inline parts are sent as regular attachments (callers
/// normally drop them, matching Gmail web).
pub(super) fn finalize_message(
    mb: mail_builder::MessageBuilder<'_>,
    body: impl Into<String>,
    html: bool,
    attachments: &[Attachment],
) -> Result<String, GwsError> {
    use mail_builder::mime::MimePart;

    let body_str = body.into();

    let (inline, regular): (Vec<&Attachment>, Vec<&Attachment>) = if html {
        attachments.iter().partition(|a| a.is_inline())
    } else {
        (Vec::new(), attachments.iter().collect())
    };

    let content: MimePart<'_> = if html {
        let text_alternative = html_to_text(&body_str)?;
        let html_part = MimePart::new("text/html", body_str);
        let html_side = if inline.is_empty() {
            html_part
        } else {
            let mut related_parts = vec![html_part];
            for att in &inline {
                let Some(cid) = att.content_id.as_deref() else {
                    return Err(GwsError::other(format!(
                        "inline part '{}' has no Content-ID",
                        att.filename
                    )));
                };
                related_parts.push(
                    MimePart::new(att.content_type.as_str(), att.data.as_slice())
                        .inline()
                        .cid(cid),
                );
            }
            MimePart::new("multipart/related", related_parts)
        };
        MimePart::new(
            "multipart/alternative",
            vec![MimePart::new("text/plain", text_alternative), html_side],
        )
    } else {
        MimePart::new("text/plain", body_str)
    };

    let root = if regular.is_empty() {
        content
    } else {
        let mut mixed_parts = vec![content];
        for att in &regular {
            mixed_parts.push(
                MimePart::new(att.content_type.as_str(), att.data.as_slice())
                    .attachment(att.filename.as_str()),
            );
        }
        MimePart::new("multipart/mixed", mixed_parts)
    };

    mb.body(root)
        .write_to_string()
        .map_err(|e| GwsError::other(format!("Failed to serialize email: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::{extract_header, strip_qp_soft_breaks};

    #[test]
    fn test_set_threading_headers_output() {
        let refs = vec![
            "ref-1@example.com".to_string(),
            "ref-2@example.com".to_string(),
        ];
        let threading = ThreadingHeaders {
            in_reply_to: "reply-to@example.com",
            references: &refs,
        };
        let mb = mail_builder::MessageBuilder::new();
        let mb = mb
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test")
            .text_body("body");
        let mb = set_threading_headers(mb, &threading).unwrap();
        let raw = mb.write_to_string().unwrap();

        let in_reply_to = extract_header(&raw, "In-Reply-To").unwrap();
        assert!(in_reply_to.contains("reply-to@example.com"));

        let references = extract_header(&raw, "References").unwrap();
        assert!(references.contains("ref-1@example.com"));
        assert!(references.contains("ref-2@example.com"));
    }

    #[test]
    fn set_threading_headers_rejects_ids_that_break_the_header() {
        for bad in [
            "",
            "a<b@example.com",
            "a>b@example.com",
            "a b@example.com",
            "a\rb",
        ] {
            let ok = vec!["ref@example.com".to_string()];
            let threading = ThreadingHeaders {
                in_reply_to: bad,
                references: &ok,
            };
            let err = set_threading_headers(mail_builder::MessageBuilder::new(), &threading)
                .err()
                .unwrap_or_else(|| panic!("{bad:?} accepted as In-Reply-To"));
            assert!(matches!(err, GwsError::Validation(_)), "{err:?}");

            let refs = vec!["ok@example.com".to_string(), bad.to_string()];
            let threading = ThreadingHeaders {
                in_reply_to: "ok@example.com",
                references: &refs,
            };
            assert!(
                set_threading_headers(mail_builder::MessageBuilder::new(), &threading).is_err(),
                "{bad:?} accepted in References"
            );
        }
    }

    #[test]
    fn test_build_references_chain() {
        // Empty references + message ID
        let original = OriginalMessage {
            message_id: "msg-1@example.com".to_string(),
            ..Default::default()
        };
        assert_eq!(build_references_chain(&original), vec!["msg-1@example.com"]);

        // Existing references + message ID
        let original = OriginalMessage {
            message_id: "msg-2@example.com".to_string(),
            references: vec![
                "msg-0@example.com".to_string(),
                "msg-1@example.com".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_references_chain(&original),
            vec![
                "msg-0@example.com",
                "msg-1@example.com",
                "msg-2@example.com"
            ]
        );

        // Empty message ID doesn't add to chain
        let original = OriginalMessage {
            message_id: String::new(),
            references: vec!["msg-0@example.com".to_string()],
            ..Default::default()
        };
        assert_eq!(build_references_chain(&original), vec!["msg-0@example.com"]);
    }

    #[test]
    fn test_attachment_single_file() {
        let att = Attachment {
            filename: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            data: b"fake pdf data".to_vec(),
            content_id: None,
        };
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &[att]).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        assert!(raw.contains("application/pdf"));
        assert!(raw.contains("Body"));
    }

    #[test]
    fn test_attachment_multiple_files() {
        let attachments = vec![
            Attachment {
                filename: "a.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"pdf data".to_vec(),
                content_id: None,
            },
            Attachment {
                filename: "b.csv".to_string(),
                content_type: "text/csv".to_string(),
                data: b"csv data".to_vec(),
                content_id: None,
            },
        ];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &attachments).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("a.pdf"));
        assert!(raw.contains("b.csv"));
    }

    #[test]
    fn test_attachment_with_html_body() {
        let att = Attachment {
            filename: "image.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
            content_id: None,
        };
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "<p>Hello</p>", true, &[att]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(raw.contains("multipart/mixed"));
        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>Hello</p>"));
        assert!(raw.contains("image.png"));
    }

    #[test]
    fn test_attachment_empty_produces_no_multipart() {
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &[]).unwrap();

        assert!(!raw.contains("multipart/mixed"));
        assert!(raw.contains("text/plain"));
    }

    #[test]
    fn test_finalize_message_html_inline_creates_multipart_related() {
        let attachments = vec![Attachment {
            filename: "photo.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
            content_id: Some("img1@example.com".to_string()),
        }];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(
            mb,
            "<p>See <img src=\"cid:img1@example.com\"></p>",
            true,
            &attachments,
        )
        .unwrap();

        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("text/html"));
        assert!(raw.contains("Content-ID: <img1@example.com>"));
        // Should NOT be multipart/mixed since there are no regular attachments
        assert!(!raw.contains("multipart/mixed"));
    }

    #[test]
    fn test_finalize_message_html_inline_and_attachment() {
        let attachments = vec![
            Attachment {
                filename: "photo.png".to_string(),
                content_type: "image/png".to_string(),
                data: vec![0x89, 0x50],
                content_id: Some("img1@example.com".to_string()),
            },
            Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"pdf data".to_vec(),
                content_id: None,
            },
        ];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "<p>HTML body</p>", true, &attachments).unwrap();

        // Should have multipart/mixed wrapping multipart/related + regular attachment
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <img1@example.com>"));
        assert!(raw.contains("report.pdf"));
    }

    #[test]
    fn test_finalize_message_plain_text_downgrades_inline_to_attachment() {
        let attachments = vec![Attachment {
            filename: "photo.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![0x89, 0x50],
            content_id: Some("img1@example.com".to_string()),
        }];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Plain text body", false, &attachments).unwrap();

        // Should NOT use multipart/related in plain text mode
        assert!(!raw.contains("multipart/related"));
        // Should be a regular attachment
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("photo.png"));
        // Content-ID should NOT appear
        assert!(!raw.contains("Content-ID: <img1@example.com>"));
    }

    #[test]
    fn test_finalize_message_html_multiple_inline_images() {
        let attachments = vec![
            Attachment {
                filename: "img1.png".to_string(),
                content_type: "image/png".to_string(),
                data: vec![0x89, 0x50],
                content_id: Some("img1@example.com".to_string()),
            },
            Attachment {
                filename: "img2.jpg".to_string(),
                content_type: "image/jpeg".to_string(),
                data: vec![0xFF, 0xD8],
                content_id: Some("img2@example.com".to_string()),
            },
        ];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(
            mb,
            "<p><img src=\"cid:img1@example.com\"><img src=\"cid:img2@example.com\"></p>",
            true,
            &attachments,
        )
        .unwrap();

        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <img1@example.com>"));
        assert!(raw.contains("Content-ID: <img2@example.com>"));
    }

    #[test]
    fn test_finalize_message_html_is_multipart_alternative_with_text_part() {
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "<p>Hello <b>there</b></p>", true, &[]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);
        assert!(raw.contains("multipart/alternative"));
        let text_pos = decoded.find("text/plain").unwrap();
        let html_pos = decoded.find("text/html").unwrap();
        assert!(text_pos < html_pos, "text/plain must precede text/html");
        assert!(decoded.contains("<p>Hello <b>there</b></p>"));
        assert!(!raw.contains("multipart/mixed"));
    }

    #[test]
    fn test_finalize_message_plain_text_has_no_alternative() {
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Just text", false, &[]).unwrap();
        assert!(!raw.contains("multipart/alternative"));
        assert!(raw.contains("text/plain"));
    }
}
