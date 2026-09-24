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

//! HTML escaping and Gmail-style attribution/quote formatting.

use super::prelude::*;

/// Line width used when rendering HTML bodies as plain text.
const TEXT_RENDER_WIDTH: usize = 78;

/// Render an HTML body as readable plain text (used for the `text/plain`
/// alternative of HTML messages and for reading HTML-only messages).
pub(super) fn html_to_text(html: &str) -> Result<String, GwsError> {
    html2text::config::plain()
        .string_from_read(html.as_bytes(), TEXT_RENDER_WIDTH)
        .map_err(|e| GwsError::other(format!("Failed to render HTML body as text: {e}")))
}

/// Resolve the HTML body for quoting or forwarding: use the original HTML
/// body if available, otherwise escape the plain text and convert newlines
/// to `<br>` tags.
pub(super) fn resolve_html_body(original: &OriginalMessage) -> String {
    match &original.body_html {
        Some(html) => html.clone(),
        None => html_escape(&original.body_text)
            .lines()
            .collect::<Vec<_>>()
            .join("<br>\r\n"),
    }
}

/// Escape `&`, `<`, `>`, `"`, `'` for safe embedding in HTML.
pub(super) fn html_escape(text: &str) -> String {
    // `&` must be replaced first to avoid double-escaping the other replacements.
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Wrap an email address in an HTML mailto link: `<a href="mailto:e">e</a>`.
///
/// The email is percent-encoded in the href to prevent mailto parameter
/// injection (e.g., `?cc=evil@example.com`) and HTML-escaped in the display text.
pub(super) fn format_email_link(email: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    let url_encoded = utf8_percent_encode(email, NON_ALPHANUMERIC);
    let display_escaped = html_escape(email);
    format!("<a href=\"mailto:{url_encoded}\">{display_escaped}</a>")
}

/// Format a `Mailbox` for the reply attribution line with a mailto link.
/// `Mailbox { name: Some("Alice"), email: "alice@example.com" }` →
/// `Alice &lt;<a href="mailto:alice%40example%2Ecom">alice@example.com</a>&gt;`
pub(super) fn format_sender_for_attribution(mailbox: &Mailbox) -> String {
    match &mailbox.name {
        Some(name) => format!(
            "{} &lt;{}&gt;",
            html_escape(name),
            format_email_link(&mailbox.email),
        ),
        None => format_email_link(&mailbox.email),
    }
}

/// Format a slice of mailboxes with mailto links on each address.
/// Used for forward To/CC fields in HTML mode.
pub(super) fn format_address_list_with_links(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(format_sender_for_attribution)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reformat an RFC 2822 date to Gmail's human-friendly attribution style:
/// `"Wed, Mar 4, 2026 at 3:01\u{202f}PM"` (`\u{202f}` = narrow no-break space
/// before AM/PM). Falls back to the raw date (HTML-escaped) if chrono cannot
/// parse it.
pub(super) fn format_date_for_attribution(raw_date: &str) -> String {
    chrono::DateTime::parse_from_rfc2822(raw_date)
        .map(|dt| html_escape(&dt.format("%a, %b %-d, %Y at %-I:%M\u{202f}%p").to_string()))
        .unwrap_or_else(|e| {
            eprintln!(
                "Note: could not parse date as RFC 2822 ({}); using raw value.",
                sanitize_for_terminal(&e.to_string())
            );
            html_escape(raw_date)
        })
}

/// Format the From line for a forwarded message using Gmail's `gmail_sendername` structure.
/// When the address has a display name, it is shown in `<strong>` with the email in a mailto
/// link. Bare emails appear in both positions (matching Gmail's behavior).
pub(super) fn format_forward_from(mailbox: &Mailbox) -> String {
    let display = match &mailbox.name {
        Some(name) => name.as_str(),
        None => &mailbox.email,
    };
    format!(
        "<strong class=\"gmail_sendername\" dir=\"auto\">{}</strong> \
         <span dir=\"auto\">&lt;{}&gt;</span>",
        html_escape(display),
        format_email_link(&mailbox.email),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text_strips_tags() {
        let text = html_to_text("<p>Hello <b>world</b></p><p>Second</p>").unwrap();
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(text.contains("Second"));
        assert!(!text.contains("<p>"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("Hello World"), "Hello World");
        assert_eq!(
            html_escape("Tom & Jerry <tj@example.com>"),
            "Tom &amp; Jerry &lt;tj@example.com&gt;"
        );
        assert_eq!(
            html_escape("He said \"hello\""),
            "He said &quot;hello&quot;"
        );
        assert_eq!(html_escape("it's"), "it&#39;s");
        assert_eq!(html_escape(""), "");
        assert_eq!(
            html_escape("a & b < c > d \"e\" f'g"),
            "a &amp; b &lt; c &gt; d &quot;e&quot; f&#39;g"
        );
    }

    #[test]
    fn test_resolve_html_body_uses_html_when_present() {
        let original = OriginalMessage {
            body_text: "ignored".to_string(),
            body_html: Some("<p>Real HTML</p>".to_string()),
            ..OriginalMessage::dry_run_placeholder("test")
        };
        assert_eq!(resolve_html_body(&original), "<p>Real HTML</p>");
    }

    #[test]
    fn test_resolve_html_body_escapes_plain_text_fallback() {
        let original = OriginalMessage {
            body_text: "Line 1 & <tag>\nLine 2\r\nLine 3".to_string(),
            body_html: None,
            ..OriginalMessage::dry_run_placeholder("test")
        };
        let result = resolve_html_body(&original);
        assert_eq!(
            result,
            "Line 1 &amp; &lt;tag&gt;<br>\r\nLine 2<br>\r\nLine 3"
        );
    }

    #[test]
    fn test_format_sender_for_attribution() {
        // Bare email
        let bare = Mailbox::parse("alice@example.com");
        assert_eq!(
            format_sender_for_attribution(&bare),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        // Name <email>
        let named = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(
            format_sender_for_attribution(&named),
            "Alice Smith &lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;"
        );
        // Special chars in name
        let special = Mailbox::parse("O'Brien & Co <ob@example.com>");
        assert_eq!(
            format_sender_for_attribution(&special),
            "O&#39;Brien &amp; Co &lt;<a href=\"mailto:ob%40example%2Ecom\">ob@example.com</a>&gt;"
        );
    }

    #[test]
    fn test_format_email_link_prevents_mailto_injection() {
        // A crafted email with ?cc= must be percent-encoded in the href so the
        // browser does not interpret it as a mailto parameter.
        let link = format_email_link("user@example.com?cc=evil@attacker.com");
        assert!(link.contains("mailto:"));
        // The href must not contain raw ?cc= (it should be percent-encoded)
        assert!(!link.contains("mailto:user@example.com?cc="));
        assert!(link.contains("%3F")); // ? encoded
        assert!(link.contains("%3D")); // = encoded
    }

    #[test]
    fn test_format_address_list_with_links() {
        let single = vec![Mailbox::parse("alice@example.com")];
        assert_eq!(
            format_address_list_with_links(&single),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        let multi = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        assert_eq!(
            format_address_list_with_links(&multi),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>, \
             <a href=\"mailto:bob%40example%2Ecom\">bob@example.com</a>"
        );
        let with_name = Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#);
        assert_eq!(
            format_address_list_with_links(&with_name),
            "Doe, John &lt;<a href=\"mailto:john%40example%2Ecom\">john@example.com</a>&gt;, \
             <a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        assert_eq!(format_address_list_with_links(&[]), "");
    }

    #[test]
    fn test_format_date_for_attribution() {
        assert_eq!(
            format_date_for_attribution("Wed, 04 Mar 2026 15:01:00 +0000"),
            "Wed, Mar 4, 2026 at 3:01\u{202f}PM"
        );
        assert_eq!(
            format_date_for_attribution("Jan 1 <2026>"),
            "Jan 1 &lt;2026&gt;"
        );
    }

    #[test]
    fn test_format_forward_from() {
        let named = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(
            format_forward_from(&named),
            "<strong class=\"gmail_sendername\" dir=\"auto\">Alice Smith</strong> \
             <span dir=\"auto\">&lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;</span>"
        );
        let bare = Mailbox::parse("alice@example.com");
        assert_eq!(
            format_forward_from(&bare),
            "<strong class=\"gmail_sendername\" dir=\"auto\">alice@example.com</strong> \
             <span dir=\"auto\">&lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;</span>"
        );
    }
}
