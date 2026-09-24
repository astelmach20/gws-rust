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

//! `gmail +send`: compose and send (or draft) a new message.

use super::dispatch::{Delivery, deliver};
use super::prelude::*;
use super::sender::resolve_sender;

/// Handle the `+send` subcommand.
pub(super) async fn handle_send(matches: &ArgMatches) -> Result<(), GwsError> {
    let mut config = parse_send_args(matches)?;
    let delivery = Delivery::from_matches(matches)?;
    delivery.confirm(
        matches,
        &format!("send an email to {}", describe(&config.to)),
    )?;

    let api = if delivery.dry_run {
        None
    } else {
        let api = super::api::authenticated(&[GMAIL_SCOPE]).await?;
        config.from = resolve_sender(&api, config.from.as_deref()).await?;
        Some(api)
    };

    let raw = create_send_raw_message(&config)?;
    deliver(api.as_ref(), delivery, &raw, None).await
}

/// Comma-separated recipient list for confirmation prompts.
pub(super) fn describe(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(|m| m.email.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) struct SendConfig {
    pub to: Vec<Mailbox>,
    pub subject: String,
    pub body: String,
    pub from: Option<Vec<Mailbox>>,
    pub cc: Option<Vec<Mailbox>>,
    pub bcc: Option<Vec<Mailbox>>,
    pub html: bool,
    pub attachments: Vec<Attachment>,
}

fn create_send_raw_message(config: &SendConfig) -> Result<String, GwsError> {
    let mb = mail_builder::MessageBuilder::new()
        .to(to_mb_address_list(&config.to))
        .subject(&config.subject);

    let mb = apply_optional_headers(
        mb,
        config.from.as_deref(),
        config.cc.as_deref(),
        config.bcc.as_deref(),
    );

    finalize_message(mb, &config.body, config.html, &config.attachments)
}

fn parse_send_args(matches: &ArgMatches) -> Result<SendConfig, GwsError> {
    let to = Mailbox::parse_list(&super::cli::required_str(matches, "to")?);
    if to.is_empty() {
        return Err(GwsError::Validation(
            "--to must specify at least one recipient".to_string(),
        ));
    }
    Ok(SendConfig {
        to,
        subject: super::cli::required_str(matches, "subject")?,
        body: super::cli::required_str(matches, "body")?,
        from: super::cli::parse_optional_mailboxes(matches, "from")?,
        cc: super::cli::parse_optional_mailboxes(matches, "cc")?,
        bcc: super::cli::parse_optional_mailboxes(matches, "bcc")?,
        html: crate::args::flag(matches, "html")?,
        attachments: parse_attachments(matches)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::{
        extract_header, helper_matches, strip_qp_soft_breaks,
    };

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let mut full = vec!["+send"];
        full.extend_from_slice(&args[1..]);
        helper_matches(&full)
    }

    #[test]
    fn test_parse_send_args() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.to.len(), 1);
        assert_eq!(config.to[0].email, "me@example.com");
        assert_eq!(config.subject, "Hi");
        assert_eq!(config.body, "Body");
        assert!(config.from.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_send_args_with_from() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "alias@example.com",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.from.as_ref().unwrap()[0].email, "alias@example.com");

        // Whitespace-only --from becomes None
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "  ",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.from.is_none());
    }

    #[test]
    fn test_parse_send_args_with_cc_and_bcc() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--cc",
            "carol@example.com",
            "--bcc",
            "secret@example.com",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.cc.as_ref().unwrap()[0].email, "carol@example.com");
        assert_eq!(config.bcc.as_ref().unwrap()[0].email, "secret@example.com");

        // Whitespace-only values become None
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--cc",
            "  ",
            "--bcc",
            "",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_send_args_html_flag() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "<b>Bold</b>",
            "--html",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.html);

        // Default is false
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Plain",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(!config.html);
    }

    #[test]
    fn test_parse_send_args_empty_to_returns_error() {
        let matches = make_matches_send(&["test", "--to", "", "--subject", "Hi", "--body", "Body"]);
        let err = parse_send_args(&matches).err().unwrap();
        assert!(
            err.to_string().contains("--to"),
            "error should mention --to"
        );
    }

    #[test]
    fn test_send_html_raw_message() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "HTML test".to_string(),
            body: "<p>Hello <b>world</b></p>".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: true,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(
            extract_header(&raw, "To")
                .unwrap()
                .contains("bob@example.com")
        );
        assert!(
            extract_header(&raw, "Subject")
                .unwrap()
                .contains("HTML test")
        );
        assert!(decoded.contains("<p>Hello <b>world</b></p>"));
        assert!(extract_header(&raw, "Cc").is_none());
    }

    #[test]
    fn test_send_plain_text_raw_message() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Hello".to_string(),
            body: "World".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(
            extract_header(&raw, "To")
                .unwrap()
                .contains("bob@example.com")
        );
        assert!(extract_header(&raw, "Subject").unwrap().contains("Hello"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("World"));
    }

    #[test]
    fn test_send_with_cc_and_bcc() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: Some(Mailbox::parse_list("carol@example.com")),
            bcc: Some(Mailbox::parse_list("secret@example.com")),
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(
            extract_header(&raw, "To")
                .unwrap()
                .contains("alice@example.com")
        );
        assert!(
            extract_header(&raw, "Cc")
                .unwrap()
                .contains("carol@example.com")
        );
        assert!(
            extract_header(&raw, "Bcc")
                .unwrap()
                .contains("secret@example.com")
        );
        // Verify no leakage between headers
        assert!(
            !extract_header(&raw, "To")
                .unwrap()
                .contains("carol@example.com")
        );
        assert!(
            !extract_header(&raw, "To")
                .unwrap()
                .contains("secret@example.com")
        );
    }

    #[test]
    fn test_send_with_from() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: Some(Mailbox::parse_list("alias@example.com")),
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(
            extract_header(&raw, "From")
                .unwrap()
                .contains("alias@example.com")
        );
        assert!(
            extract_header(&raw, "To")
                .unwrap()
                .contains("bob@example.com")
        );
    }

    #[test]
    fn test_send_without_from_has_no_from_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "From").is_none());
    }

    #[test]
    fn test_send_multiple_to_recipients() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com, bob@example.com"),
            subject: "Group".to_string(),
            body: "Hi all".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();
        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("alice@example.com"));
        assert!(to_header.contains("bob@example.com"));
    }

    #[test]
    fn test_send_crlf_injection_in_from_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: Some(Mailbox::parse_list(
                "sender@example.com\r\nBcc: evil@attacker.com",
            )),
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        // The CRLF injection should not create a Bcc header
        assert!(
            extract_header(&raw, "Bcc").is_none(),
            "CRLF injection via --from should not create Bcc header"
        );
        // The From header should contain the sanitized email
        assert!(
            extract_header(&raw, "From")
                .unwrap()
                .contains("sender@example.com")
        );
    }

    #[test]
    fn test_send_crlf_injection_in_cc_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: Some(Mailbox::parse_list("carol@example.com\r\nX-Injected: yes")),
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        // CRLF stripped → "X-Injected: yes" is concatenated into the email,
        // not emitted as a separate header line
        assert!(
            extract_header(&raw, "X-Injected").is_none(),
            "CRLF injection via --cc should not create X-Injected header"
        );
        assert!(
            extract_header(&raw, "Cc")
                .unwrap()
                .contains("carol@example.com")
        );
    }

    #[test]
    fn test_send_with_attachment_produces_multipart() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Report".to_string(),
            body: "See attached".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"fake pdf".to_vec(),
                content_id: None,
            }],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        assert!(raw.contains("See attached"));
        assert!(
            extract_header(&raw, "To")
                .unwrap()
                .contains("alice@example.com")
        );
    }
}
