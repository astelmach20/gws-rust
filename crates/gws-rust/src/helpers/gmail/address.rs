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

//! RFC 5322 mailbox parsing and conversion to `mail_builder` addresses.

use super::prelude::*;

/// Strip ASCII control characters (0x00–0x1F, 0x7F) from a string.
///
/// Defense-in-depth: mail-builder uses structured types for headers which
/// prevents most injection, but email addresses are written as raw bytes
/// inside angle brackets. Stripping control characters at the parse boundary
/// closes any residual CRLF/null-byte injection vectors before data reaches
/// mail-builder.
pub(super) fn sanitize_control_chars(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_control()).collect()
}

/// A parsed RFC 5322 mailbox: optional display name + email address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Mailbox {
    pub name: Option<String>,
    pub email: String,
}

impl Mailbox {
    /// Parse a single address like `"Alice <alice@example.com>"` or `"alice@example.com"`.
    ///
    /// Intentionally total (never fails): this parses both user CLI input and
    /// Gmail API header values. API headers are already server-validated, so
    /// returning `Result` would force unnecessary error handling at every parse site.
    /// User-input validation happens at the `Config` boundary (non-empty `--to`);
    /// syntactic email validation is left to the Gmail API.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if let Some(start) = raw.rfind('<')
            && let Some(end) = raw[start..].find('>')
        {
            let email = sanitize_control_chars(raw[start + 1..start + end].trim());
            let name_part = raw[..start].trim();
            let name = if name_part.is_empty() {
                None
            } else {
                // Strip surrounding quotes: "Alice Smith" → Alice Smith
                let unquoted = name_part
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(name_part);
                Some(sanitize_control_chars(unquoted))
            };
            return Self { name, email };
        }
        Self {
            name: None,
            email: sanitize_control_chars(raw),
        }
    }

    /// Parse a comma-separated address list, respecting quoted strings.
    /// Empty-email entries (e.g. from trailing commas) are filtered out.
    pub fn parse_list(raw: &str) -> Vec<Self> {
        split_raw_mailbox_list(raw)
            .into_iter()
            .map(Mailbox::parse)
            .filter(|m| !m.email.is_empty())
            .collect()
    }

    /// Lowercase email for case-insensitive comparison.
    pub fn email_lowercase(&self) -> String {
        self.email.to_lowercase()
    }
}

/// Display format for logging and plain-text message bodies (not RFC 5322 headers).
/// Does not quote display names containing specials; mail-builder handles header serialization.
impl std::fmt::Display for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} <{}>", name, self.email),
            None => write!(f, "{}", self.email),
        }
    }
}

/// Convert a single `Mailbox` to a `mail_builder::Address`.
pub(super) fn to_mb_address(mailbox: &Mailbox) -> MbAddress<'_> {
    MbAddress::new_address(mailbox.name.as_deref(), &mailbox.email)
}

/// Convert a slice of `Mailbox` to a `mail_builder::Address` (list).
pub(super) fn to_mb_address_list(mailboxes: &[Mailbox]) -> MbAddress<'_> {
    MbAddress::new_list(mailboxes.iter().map(to_mb_address).collect())
}

/// Strip angle brackets from a message ID: `"<abc@example.com>"` → `"abc@example.com"`.
pub(super) fn strip_angle_brackets(id: &str) -> &str {
    id.trim()
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(id.trim())
}

/// Split an RFC 5322 mailbox list on commas, respecting quoted strings.
/// Returns raw string slices — use `Mailbox::parse_list` for structured parsing.
pub(super) fn split_raw_mailbox_list(header: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let mut prev_backslash = false;

    for (i, ch) in header.char_indices() {
        match ch {
            '\\' if in_quotes => {
                prev_backslash = !prev_backslash;
                continue;
            }
            '"' if !prev_backslash => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let token = header[start..i].trim();
                if !token.is_empty() {
                    result.push(token);
                }
                start = i + 1;
            }
            _ => {}
        }
        prev_backslash = false;
    }

    let token = header[start..].trim();
    if !token.is_empty() {
        result.push(token);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::extract_header;

    #[test]
    fn test_to_mb_address_bare_email() {
        let mailbox = Mailbox::parse("alice@example.com");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address(&mailbox))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
    }

    #[test]
    fn test_to_mb_address_with_display_name() {
        let mailbox = Mailbox::parse("Alice Smith <alice@example.com>");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address(&mailbox))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
        assert!(to.contains("Alice Smith"));
    }

    #[test]
    fn test_to_mb_address_list_multiple() {
        let mailboxes = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address_list(&mailboxes))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
        assert!(to.contains("bob@example.com"));
        assert!(to.contains("Bob"));
    }

    #[test]
    fn test_mailbox_parse_bare_email() {
        let m = Mailbox::parse("alice@example.com");
        assert_eq!(m.email, "alice@example.com");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_with_display_name() {
        let m = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(m.email, "alice@example.com");
        assert_eq!(m.name.as_deref(), Some("Alice Smith"));
    }

    #[test]
    fn test_mailbox_parse_quoted_display_name() {
        let m = Mailbox::parse("\"Bob, Jr.\" <bob@example.com>");
        assert_eq!(m.email, "bob@example.com");
        assert_eq!(m.name.as_deref(), Some("Bob, Jr."));
    }

    #[test]
    fn test_mailbox_parse_malformed_no_closing_bracket() {
        let m = Mailbox::parse("Alice <alice@example.com");
        assert_eq!(m.email, "Alice <alice@example.com");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_empty() {
        let m = Mailbox::parse("");
        assert_eq!(m.email, "");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_empty_angle_brackets() {
        let m = Mailbox::parse("Alice <>");
        // Empty email inside angle brackets
        assert_eq!(m.email, "");
        assert_eq!(m.name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_mailbox_parse_strips_crlf_injection_in_email() {
        let m = Mailbox::parse("foo@bar.com\r\nBcc: evil@attacker.com");
        assert_eq!(m.email, "foo@bar.comBcc: evil@attacker.com");
        assert!(!m.email.contains('\r'));
        assert!(!m.email.contains('\n'));
    }

    #[test]
    fn test_mailbox_parse_strips_crlf_injection_in_angle_bracket_email() {
        let m = Mailbox::parse("Alice <foo@bar.com\r\nBcc: evil@attacker.com>");
        assert!(!m.email.contains('\r'));
        assert!(!m.email.contains('\n'));
        assert!(m.email.contains("foo@bar.com"));
    }

    #[test]
    fn test_mailbox_parse_strips_control_chars_from_name() {
        let m = Mailbox::parse("Alice\0Bob <alice@example.com>");
        assert_eq!(m.name.as_deref(), Some("AliceBob"));
        assert!(!m.name.unwrap().contains('\0'));
    }

    #[test]
    fn test_mailbox_parse_strips_null_bytes_from_email() {
        let m = Mailbox::parse("alice\0@example.com");
        assert_eq!(m.email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_strips_tab_from_email() {
        let m = Mailbox::parse("alice\t@example.com");
        assert_eq!(m.email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_non_ascii_display_name() {
        let m = Mailbox::parse("田中太郎 <tanaka@example.com>");
        assert_eq!(m.email, "tanaka@example.com");
        assert_eq!(m.name.as_deref(), Some("田中太郎"));

        // Verify non-ASCII name flows through to mail-builder without panic
        // and gets RFC 2047 encoded (replacing hand-rolled encode_address_header from #482)
        let mb = mail_builder::MessageBuilder::new()
            .to(to_mb_address(&m))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        assert!(raw.contains("tanaka@example.com"));
        assert!(!raw.contains("田中太郎")); // raw CJK should be RFC 2047 encoded
        assert!(raw.contains("=?utf-8?")); // encoded-word present
    }

    #[test]
    fn test_mailbox_parse_list() {
        let list = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "alice@example.com");
        assert_eq!(list[1].email, "bob@example.com");
        assert_eq!(list[1].name.as_deref(), Some("Bob"));
    }

    #[test]
    fn test_mailbox_parse_list_with_quoted_comma() {
        let list = Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "john@example.com");
        assert_eq!(list[0].name.as_deref(), Some("Doe, John"));
        assert_eq!(list[1].email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_list_filters_empty_emails() {
        // Empty string → empty vec
        assert!(Mailbox::parse_list("").is_empty());

        // Whitespace-only commas → empty vec
        assert!(Mailbox::parse_list("  ,  ,  ").is_empty());

        // Trailing comma → no phantom entry
        let list = Mailbox::parse_list("alice@example.com,");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "alice@example.com");

        // Leading comma
        let list = Mailbox::parse_list(",alice@example.com");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "alice@example.com");

        // Empty angle brackets filtered
        let list = Mailbox::parse_list("Alice <>, bob@example.com");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "bob@example.com");
    }

    #[test]
    fn test_mailbox_display() {
        let bare = Mailbox {
            name: None,
            email: "alice@example.com".to_string(),
        };
        assert_eq!(bare.to_string(), "alice@example.com");

        let named = Mailbox {
            name: Some("Alice".to_string()),
            email: "alice@example.com".to_string(),
        };
        assert_eq!(named.to_string(), "Alice <alice@example.com>");
    }

    /// Regression test for PR #513: display names with RFC 2822 special characters
    /// (commas, parens, colons, etc.) must be properly quoted in the To: header
    /// so Gmail does not reject them with "Invalid To header".
    #[test]
    fn test_rfc2822_display_name_quoting_via_mail_builder() {
        let test_cases = [
            ("Anderson, Rich (CORP)", "rich@example.com", "comma/parens"),
            ("Dr. Smith: Chief", "smith@example.com", "colon"),
            ("O'Brien & Co.", "ob@example.com", "dot/ampersand"),
        ];

        for (name, email, description) in test_cases {
            let m = Mailbox {
                name: Some(name.to_string()),
                email: email.to_string(),
            };
            let raw = mail_builder::MessageBuilder::new()
                .to(to_mb_address(&m))
                .subject("test")
                .text_body("body")
                .write_to_string()
                .unwrap();
            let to_line = raw
                .lines()
                .find(|l| l.starts_with("To:"))
                .unwrap_or_else(|| panic!("No To: header for case: {description}"));

            let quoted = format!("\"{name}\"");
            assert!(
                to_line.contains(&quoted) || to_line.contains("=?utf-8?"),
                "Display name with {description} must be quoted: {to_line}"
            );
        }
    }

    #[test]
    fn test_strip_angle_brackets() {
        assert_eq!(strip_angle_brackets("<abc@example.com>"), "abc@example.com");
        assert_eq!(strip_angle_brackets("abc@example.com"), "abc@example.com");
        assert_eq!(
            strip_angle_brackets("  <abc@example.com>  "),
            "abc@example.com"
        );
    }

    #[test]
    fn test_split_raw_mailbox_list() {
        assert_eq!(
            split_raw_mailbox_list("alice@example.com, bob@example.com"),
            vec!["alice@example.com", "bob@example.com"]
        );
        assert_eq!(
            split_raw_mailbox_list("alice@example.com"),
            vec!["alice@example.com"]
        );
        assert!(split_raw_mailbox_list("").is_empty());
        assert_eq!(
            split_raw_mailbox_list(r#""Doe, John" <john@example.com>, alice@example.com"#),
            vec![r#""Doe, John" <john@example.com>"#, "alice@example.com"]
        );
        assert_eq!(
            split_raw_mailbox_list(r#""Doe \"JD, Sr\"" <john@example.com>, alice@example.com"#),
            vec![
                r#""Doe \"JD, Sr\"" <john@example.com>"#,
                "alice@example.com"
            ]
        );
        assert_eq!(
            split_raw_mailbox_list(r#""Trail\\" <t@example.com>, b@example.com"#),
            vec![r#""Trail\\" <t@example.com>"#, "b@example.com"]
        );
    }
}
