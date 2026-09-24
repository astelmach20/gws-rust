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

//! `gmail +read`: print a message's body (and optionally headers).

use super::cli::required_str;
use super::prelude::*;
use crate::helpers::modelarmor::{SanitizeConfig, require_pass, sanitize_value};
use std::io::{self, Write};

#[derive(Debug, Clone, Copy)]
struct ReadOptions {
    json: bool,
    headers: bool,
    html: bool,
}

/// Handle the `+read` subcommand.
pub(super) async fn handle_read(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    let message_id = required_str(matches, "message-id")?;
    let opts = ReadOptions {
        json: required_str(matches, "body-format")? == "json",
        headers: matches.get_flag("headers"),
        html: matches.get_flag("html"),
    };

    if crate::helpers::rest::dry_run(matches)? {
        let url = format!(
            "{}/users/me/messages/{}",
            super::api::GMAIL_API_BASE,
            crate::validate::encode_path_segment(&message_id)
        );
        return crate::helpers::rest::print_dry_run(vec![crate::helpers::rest::dry_run_request(
            "GET",
            &url,
            &[("format", "full".to_string())],
            None,
        )]);
    }

    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let original = api.get_original_message(&message_id).await?;

    // Run the configured Model Armor policy over the message before printing.
    let as_json = serde_json::to_value(&original)
        .map_err(|e| other_error(format!("Failed to serialize message: {e}")))?;
    let checked = require_pass(sanitize_value(sanitize_config, as_json).await?)?;

    let mut stdout = io::stdout().lock();
    if opts.json {
        let text = serde_json::to_string_pretty(&checked)
            .map_err(|e| other_error(format!("Failed to serialize message: {e}")))?;
        writeln!(stdout, "{text}")
            .map_err(|e| other_error(format!("Failed to write output: {e}")))?;
        return Ok(());
    }
    render(&original, opts, &mut stdout)
}

/// Render a message as text or JSON to `out`.
fn render(
    original: &OriginalMessage,
    opts: ReadOptions,
    out: &mut impl Write,
) -> Result<(), GwsError> {
    let io_err = |e: io::Error| other_error(format!("Failed to write output: {e}"));

    if opts.json {
        let text = serde_json::to_string_pretty(original)
            .map_err(|e| other_error(format!("Failed to serialize message: {e}")))?;
        return writeln!(out, "{text}").map_err(io_err);
    }

    if opts.headers {
        let from_str = original.from.to_string();
        let to_str = format_mailbox_list(&original.to);
        let cc_str = original
            .cc
            .as_ref()
            .map(|cc| format_mailbox_list(cc))
            .unwrap_or_default();
        let headers_to_show: [(&str, &str); 5] = [
            ("From", &from_str),
            ("To", &to_str),
            ("Cc", &cc_str),
            ("Subject", &original.subject),
            ("Date", original.date.as_deref().unwrap_or_default()),
        ];
        for (name, value) in headers_to_show {
            if value.is_empty() {
                continue;
            }
            // Replace newlines to prevent header spoofing in the output, then sanitize.
            let sanitized_value = sanitize_for_terminal(&value.replace(['\r', '\n'], " "));
            writeln!(out, "{name}: {sanitized_value}").map_err(io_err)?;
        }
        writeln!(out, "---").map_err(io_err)?;
    }

    let body = if opts.html {
        original
            .body_html
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&original.body_text)
    } else {
        &original.body_text
    };
    writeln!(out, "{}", sanitize_for_terminal(body)).map_err(io_err)
}

/// Format a slice of Mailbox as a displayable comma-separated string.
fn format_mailbox_list(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OriginalMessage {
        OriginalMessage {
            from: Mailbox::parse("Alice <alice@example.com>"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hi\r\nX-Injected: 1".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Body text".to_string(),
            body_html: Some("<p>Body</p>".to_string()),
            ..Default::default()
        }
    }

    fn render_to_string(msg: &OriginalMessage, opts: ReadOptions) -> String {
        let mut out = Vec::new();
        render(msg, opts, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_render_text_with_headers_neutralizes_newlines() {
        let out = render_to_string(
            &sample(),
            ReadOptions {
                json: false,
                headers: true,
                html: false,
            },
        );
        assert!(out.contains("From: Alice <alice@example.com>"));
        assert!(out.contains("Subject: Hi  X-Injected: 1"));
        assert!(!out.contains("\nX-Injected"));
        assert!(out.contains("---\nBody text"));
    }

    #[test]
    fn test_render_html_body() {
        let out = render_to_string(
            &sample(),
            ReadOptions {
                json: false,
                headers: false,
                html: true,
            },
        );
        assert_eq!(out.trim(), "<p>Body</p>");
    }

    #[test]
    fn test_render_json() {
        let out = render_to_string(
            &sample(),
            ReadOptions {
                json: true,
                headers: false,
                html: false,
            },
        );
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["body_text"], "Body text");
        assert_eq!(v["from"]["email"], "alice@example.com");
    }

    #[test]
    fn test_sanitize_for_terminal() {
        let malicious = "Subject: \x1b]0;MALICIOUS\x07Hello\nWorld\r\t";
        let sanitized = sanitize_for_terminal(malicious);
        // ANSI escape sequences (control chars) should be removed
        assert!(!sanitized.contains('\x1b'));
        assert!(!sanitized.contains('\x07'));
        // CR is also stripped (can be abused for terminal overwrite attacks)
        assert!(!sanitized.contains('\r'));
        // Newline and tab should be preserved
        assert!(sanitized.contains("Hello"));
        assert!(sanitized.contains('\n'));
        assert!(sanitized.contains('\t'));
    }

    #[test]
    fn test_format_mailbox_list_empty() {
        assert_eq!(format_mailbox_list(&[]), "");
    }

    #[test]
    fn test_format_mailbox_list_single() {
        let mailboxes = Mailbox::parse_list("alice@example.com");
        let result = format_mailbox_list(&mailboxes);
        assert!(result.contains("alice@example.com"));
    }

    #[test]
    fn test_format_mailbox_list_multiple() {
        let mailboxes = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        let result = format_mailbox_list(&mailboxes);
        assert!(result.contains("alice@example.com"));
        assert!(result.contains("bob@example.com"));
    }
}
