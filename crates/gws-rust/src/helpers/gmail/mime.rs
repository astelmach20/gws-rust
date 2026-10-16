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
/// message ID (the direct parent), empty when the parent has no `Message-ID`;
/// `references` is the full ordered chain, possibly empty.
/// The references chain should be fully assembled via `build_references_chain`
/// before constructing this.
pub(super) struct ThreadingHeaders<'a> {
    pub in_reply_to: &'a str,
    pub references: &'a [String],
}

/// Build the full references chain for threading (RFC 5322 §3.6.4): the
/// parent's `References`, or, when it has none, its `In-Reply-To` if that
/// names a single message; then the parent's own message ID.
pub(super) fn build_references_chain(original: &OriginalMessage) -> Vec<String> {
    let mut refs = if !original.references.is_empty() {
        original.references.clone()
    } else if let [single] = original.in_reply_to.as_slice() {
        vec![single.clone()]
    } else {
        Vec::new()
    };
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
///
/// When the parent has no `Message-ID` (`in_reply_to` is empty), RFC 5322
/// §3.6.4 leaves the new message without `In-Reply-To`, and without
/// `References` when the parent had no chain either. The message still goes
/// out: Gmail files it in the parent's thread by thread ID, but other mail
/// clients may not thread it, which is said on stderr.
pub(super) fn set_threading_headers<'x>(
    mut mb: mail_builder::MessageBuilder<'x>,
    threading: &ThreadingHeaders<'x>,
) -> Result<mail_builder::MessageBuilder<'x>, GwsError> {
    for id in threading.references {
        validate_bare_msg_id(id)?;
    }

    use mail_builder::headers::message_id::MessageId;

    if threading.in_reply_to.is_empty() {
        tracing::warn!(
            "the original message has no Message-ID header: sending without In-Reply-To; \
             Gmail keeps the message in the same thread, other mail clients may not"
        );
    } else {
        validate_bare_msg_id(threading.in_reply_to)?;
        mb = mb.in_reply_to(MessageId::new(threading.in_reply_to));
    }
    if !threading.references.is_empty() {
        mb = mb.references(MessageId {
            id: threading
                .references
                .iter()
                .map(|id| id.as_str().into())
                .collect(),
        });
    }
    Ok(mb)
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
                related_parts.push(attachment_part(att).inline().cid(cid));
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
            mixed_parts.push(attachment_part(att).attachment(att.filename.as_str()));
        }
        MimePart::new("multipart/mixed", mixed_parts)
    };

    mb.body(root)
        .write_to_string()
        .map_err(|e| GwsError::other(format!("Failed to serialize email: {e}")))
}

/// The MIME leaf for an attachment's bytes.
///
/// RFC 2046 §5.2.1 allows only `7bit`, `8bit` or `binary` for a
/// `message/rfc822` body, but mail-builder base64-encodes every non-text part.
/// A message that is already in canonical form (CRLF line endings, lines of at
/// most 998 octets, no NUL) is therefore sent as-is with `7bit` or, when it is
/// UTF-8 with 8-bit octets, `8bit`. Anything else (bare LF line endings, long
/// lines, NUL, non-UTF-8 octets) cannot be sent unencoded without changing
/// it, so it is labelled `application/octet-stream` and base64-encoded: the
/// recipient gets the file byte for byte, under its original name.
fn attachment_part(att: &Attachment) -> mail_builder::mime::MimePart<'_> {
    use mail_builder::mime::MimePart;

    if !is_message_rfc822(&att.content_type) {
        return MimePart::new(att.content_type.as_str(), att.data.as_slice());
    }
    match unencoded_transfer_encoding(&att.data) {
        Some(encoding) => MimePart::new(att.content_type.as_str(), att.data.as_slice())
            .transfer_encoding(encoding),
        None => MimePart::new("application/octet-stream", att.data.as_slice()),
    }
}

fn is_message_rfc822(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("message/rfc822"))
}

/// `7bit` or `8bit` when `data` can be written into the message unchanged
/// (RFC 2045 §2.7/§2.8), else `None`.
fn unencoded_transfer_encoding(data: &[u8]) -> Option<&'static str> {
    const MAX_LINE: usize = 998;
    let mut line_len = 0;
    let mut bytes = data.iter().peekable();
    while let Some(&b) = bytes.next() {
        match b {
            0 | b'\n' => return None,
            b'\r' => {
                if bytes.next() != Some(&b'\n') {
                    return None;
                }
                line_len = 0;
            }
            _ => {
                line_len += 1;
                if line_len > MAX_LINE {
                    return None;
                }
            }
        }
    }
    if data.is_ascii() {
        Some("7bit")
    } else if std::str::from_utf8(data).is_ok() {
        Some("8bit")
    } else {
        None
    }
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
    fn references_chain_starts_from_in_reply_to_without_references() {
        let original = OriginalMessage {
            message_id: "parent@example.com".to_string(),
            in_reply_to: vec!["grandparent@example.com".to_string()],
            ..Default::default()
        };
        assert_eq!(
            build_references_chain(&original),
            vec!["grandparent@example.com", "parent@example.com"]
        );

        // References wins over In-Reply-To when both are present.
        let original = OriginalMessage {
            message_id: "parent@example.com".to_string(),
            references: vec!["root@example.com".to_string()],
            in_reply_to: vec!["grandparent@example.com".to_string()],
            ..Default::default()
        };
        assert_eq!(
            build_references_chain(&original),
            vec!["root@example.com", "parent@example.com"]
        );

        // An In-Reply-To naming several messages is ambiguous: not used.
        let original = OriginalMessage {
            message_id: "parent@example.com".to_string(),
            in_reply_to: vec!["a@example.com".to_string(), "b@example.com".to_string()],
            ..Default::default()
        };
        assert_eq!(
            build_references_chain(&original),
            vec!["parent@example.com"]
        );
    }

    #[test]
    fn set_threading_headers_rejects_ids_that_break_the_header() {
        for bad in [
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
    fn set_threading_headers_omits_headers_the_parent_cannot_supply() {
        let write = |threading: &ThreadingHeaders<'_>| {
            let mb = mail_builder::MessageBuilder::new()
                .to(MbAddress::new_address(None::<&str>, "test@example.com"))
                .subject("test");
            let mb = set_threading_headers(mb, threading).unwrap();
            finalize_message(mb, "b", false, &[]).unwrap()
        };

        // Parent without a Message-ID or a chain: neither header.
        let raw = write(&ThreadingHeaders {
            in_reply_to: "",
            references: &[],
        });
        assert!(extract_header(&raw, "In-Reply-To").is_none(), "{raw}");
        assert!(extract_header(&raw, "References").is_none(), "{raw}");

        // Parent without a Message-ID but with a chain: References only.
        let refs = vec!["root@example.com".to_string()];
        let raw = write(&ThreadingHeaders {
            in_reply_to: "",
            references: &refs,
        });
        assert!(extract_header(&raw, "In-Reply-To").is_none(), "{raw}");
        let references = extract_header(&raw, "References").unwrap();
        assert!(references.contains("<root@example.com>"), "{raw}");

        // An empty ID inside the chain is still rejected.
        let refs = vec![String::new()];
        let threading = ThreadingHeaders {
            in_reply_to: "ok@example.com",
            references: &refs,
        };
        assert!(set_threading_headers(mail_builder::MessageBuilder::new(), &threading).is_err());
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
    fn generated_message_id_does_not_reveal_the_local_host_name() {
        // mail-builder's default `gethostname` feature puts the machine's host
        // name (often the owner's name, e.g. "janes-macbook-pro.local") on the
        // right of every Message-ID, which ends up in dry-run plans and drafts.
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &[]).unwrap();
        let id = extract_header(&raw, "Message-ID").unwrap();
        let domain = id
            .trim()
            .strip_suffix('>')
            .and_then(|id| id.rsplit_once('@'))
            .map(|(_, domain)| domain);
        assert_eq!(domain, Some("localhost"), "Message-ID: {id}");
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
    fn message_rfc822_attachment_is_never_base64() {
        let canonical = b"From: a@example.com\r\nSubject: Hi\r\n\r\nBody\r\n".to_vec();
        let utf8 = "Subject: Caf\u{e9}\r\n\r\nCaf\u{e9}\r\n"
            .as_bytes()
            .to_vec();
        let bare_lf = b"Subject: Hi\n\nBody\n".to_vec();
        let latin1 = b"Subject: Caf\xe9\r\n\r\nBody\r\n".to_vec();
        let long_line = [b"Subject: Hi\r\n\r\n".as_slice(), &[b'x'; 999]].concat();
        let cases: [(&[u8], &str, &str); 5] = [
            (&canonical, "message/rfc822", "7bit"),
            (&utf8, "message/rfc822", "8bit"),
            (&bare_lf, "application/octet-stream", "base64"),
            (&latin1, "application/octet-stream", "base64"),
            (&long_line, "application/octet-stream", "base64"),
        ];
        for (data, want_type, want_cte) in cases {
            let att = Attachment {
                filename: "note.eml".to_string(),
                content_type: "message/rfc822".to_string(),
                data: data.to_vec(),
                content_id: None,
            };
            let mb = mail_builder::MessageBuilder::new()
                .to(MbAddress::new_address(None::<&str>, "test@example.com"))
                .subject("test");
            let Ok(raw) = finalize_message(mb, "Body", false, &[att]) else {
                panic!("{want_type} {want_cte}: not serialized");
            };
            let part = raw
                .split("--")
                .find(|p| p.contains("note.eml"))
                .unwrap_or_else(|| panic!("no attachment part: {raw}"));
            let (headers, body) = part.split_once("\r\n\r\n").unwrap();
            let headers = headers.to_ascii_lowercase();
            assert!(
                headers.contains(&format!("content-type: {want_type}")),
                "{headers}"
            );
            assert!(
                headers.contains(&format!("content-transfer-encoding: {want_cte}")),
                "{headers}"
            );
            if want_cte == "base64" {
                let joined: String = body.split_whitespace().collect();
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(joined)
                    .unwrap();
                assert_eq!(decoded, data, "{want_type}: file changed");
            } else {
                // The part body is the file, followed by the CRLF that
                // belongs to the next boundary delimiter.
                assert_eq!(body.as_bytes().strip_suffix(b"\r\n"), Some(data));
            }
        }

        // Other content types are untouched.
        assert!(is_message_rfc822("Message/RFC822; x=y"));
        assert!(!is_message_rfc822("message/global"));
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
