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

//! Shared input validation helpers.
//!
//! These functions harden inputs against adversarial or accidentally
//! malformed values — especially important when the CLI is invoked by an
//! LLM agent rather than a human operator.
//!
//! - Character and identifier validation, URL path encoding (this module)
//! - File-system path validation with an optional CWD sandbox: [`paths`]
//! - API endpoint (rootUrl) trust checks: [`validate_api_base`]
//! - Model Armor template names: [`ModelArmorTemplate`]

mod endpoint;
mod modelarmor;
mod paths;

pub use endpoint::{
    API_BASE_URL_ENV, EndpointPolicy, api_base_override, is_google_api_host,
    parse_api_base_override, validate_api_base, validate_api_base_with,
};
pub use modelarmor::ModelArmorTemplate;
pub use paths::{
    PathPolicy, RESTRICT_PATHS_ENV, resolve_dir_path, resolve_file_path, resolve_output_dir,
    validate_safe_dir_path, validate_safe_file_path, validate_safe_output_dir,
};

use crate::error::GwsError;

// ── Dangerous character detection ─────────────────────────────────────

/// Returns `true` for Unicode characters that are dangerous in terminal
/// output but not caught by `char::is_control()`: zero-width chars, bidi
/// overrides, Unicode line/paragraph separators, and directional isolates.
pub fn is_dangerous_unicode(c: char) -> bool {
    matches!(c,
        // zero-width: ZWSP, ZWNJ, ZWJ, BOM/ZWNBSP
        '\u{200B}'..='\u{200D}' | '\u{FEFF}' |
        // bidi: LRE, RLE, PDF, LRO, RLO
        '\u{202A}'..='\u{202E}' |
        // line / paragraph separators
        '\u{2028}'..='\u{2029}' |
        // directional isolates: LRI, RLI, FSI, PDI
        '\u{2066}'..='\u{2069}'
    )
}

/// Rejects strings containing control characters (C0: U+0000–U+001F,
/// C1: U+0080–U+009F, and DEL: U+007F) or dangerous Unicode characters
/// such as zero-width chars, bidi overrides, and line/paragraph separators.
///
/// Used for validating argument values at the parse boundary.
pub fn reject_dangerous_chars(value: &str, flag_name: &str) -> Result<(), GwsError> {
    for c in value.chars() {
        if c.is_control() {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid control characters"
            )));
        }
        if is_dangerous_unicode(c) {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid Unicode characters"
            )));
        }
    }
    Ok(())
}

// ── URL encoding ──────────────────────────────────────────────────────

/// Characters escaped in a path segment: everything except the RFC 3986
/// unreserved set (`A-Z a-z 0-9 - . _ ~`) and `@` (a legal `pchar`, common in
/// calendar IDs and the Tasks `@default` alias).
const PATH_SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'@');

/// Percent-encode a value for use as a single URL path segment (e.g., file ID,
/// calendar ID, message ID).
///
/// Unreserved characters stay literal: Google IDs routinely contain `-` and
/// `_`, and some endpoints (e.g. `scripts.run`, upstream #842) reject them as
/// `%2D`/`%5F`. The dot-segments `.` and `..` are always escaped so they can
/// never climb the path.
pub fn encode_path_segment(s: &str) -> String {
    if s == "." || s == ".." {
        return s.replace('.', "%2E");
    }
    percent_encoding::utf8_percent_encode(s, PATH_SEGMENT).to_string()
}

/// Percent-encode a value for use in URI path templates where `/` should stay
/// as a path separator (e.g., RFC 6570 `{+name}` expansions).
///
/// Each path segment is encoded independently, then joined with `/`, so
/// dangerous characters like `#`/`?` are still escaped while hierarchical
/// resource names such as `projects/p/locations/l` remain readable.
pub fn encode_path_preserving_slashes(s: &str) -> String {
    s.split('/')
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

// ── Resource / API validators ─────────────────────────────────────────

/// Well-known Google API aliases accepted as a whole resource name even
/// though they contain `@` (e.g. the Tasks API default list `@default`).
pub const RESOURCE_NAME_ALIASES: &[&str] = &["@default"];

/// Whether `c` is allowed in a resource name: `[A-Za-z0-9._~/-]`.
fn is_resource_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '/' | '-')
}

/// Validate a multi-segment resource name (e.g., `spaces/ABC`, `subscriptions/123`).
///
/// Only `[A-Za-z0-9._~/-]` is accepted (plus the aliases in
/// [`RESOURCE_NAME_ALIASES`]), so `@`, `\`, `:`, `%`, `?`, `#`, whitespace,
/// control characters and non-ASCII are rejected. Segments must be non-empty
/// and must not be `.` or `..`. Returns the validated name.
pub fn validate_resource_name(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "Resource name must not be empty".to_string(),
        ));
    }
    if RESOURCE_NAME_ALIASES.contains(&s) {
        return Ok(s);
    }
    let shown = s.escape_debug();
    if s.split('/').any(|seg| seg == ".." || seg == ".") {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain path traversal ('..' or '.') segments: {shown}"
        )));
    }
    if s.contains('%') {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain '%' (URL encoding bypass attempt): {shown}"
        )));
    }
    if s.contains('?') || s.contains('#') {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain '?' or '#': {shown}"
        )));
    }
    if let Some(c) = s.chars().find(|c| !is_resource_name_char(*c)) {
        return Err(GwsError::Validation(format!(
            "Resource name contains invalid characters ({:?} is not allowed; only A-Z a-z 0-9 . _ ~ / - are): {shown}",
            c
        )));
    }
    if s.split('/').any(str::is_empty) {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain empty segments (leading, trailing, or double '/'): {shown}"
        )));
    }
    Ok(s)
}

/// Maximum accepted length of an API identifier (service name or version).
pub const MAX_API_IDENTIFIER_LEN: usize = 100;

/// Validate an API identifier (service name, version string) for use in
/// cache filenames and discovery URLs.
///
/// Must be 1..=[`MAX_API_IDENTIFIER_LEN`] ASCII characters, start with an
/// alphanumeric character, contain only alphanumerics, `-`, `_` and `.`, and
/// never contain `..`. This rules out path traversal and URL injection.
pub fn validate_api_identifier(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "API identifier must not be empty".to_string(),
        ));
    }
    if s.len() > MAX_API_IDENTIFIER_LEN {
        return Err(GwsError::Validation(format!(
            "API identifier is longer than {MAX_API_IDENTIFIER_LEN} characters"
        )));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(GwsError::Validation(format!(
            "API identifier contains invalid characters (only alphanumeric, '-', '_', '.' allowed): {}",
            s.escape_debug()
        )));
    }
    if !s.starts_with(|c: char| c.is_ascii_alphanumeric()) || s.contains("..") {
        return Err(GwsError::Validation(format!(
            "API identifier must start with a letter or digit and must not contain '..': {s}"
        )));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- reject_dangerous_chars ---

    #[test]
    fn test_reject_dangerous_chars_clean() {
        assert!(reject_dangerous_chars("hello/world", "test").is_ok());
    }

    #[test]
    fn test_reject_dangerous_chars_tab() {
        assert!(reject_dangerous_chars("hello\tworld", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_newline() {
        assert!(reject_dangerous_chars("hello\nworld", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_del() {
        assert!(reject_dangerous_chars("hello\x7Fworld", "test").is_err());
    }

    // -- encode_path_segment --------------------------------------------------

    #[test]
    fn test_encode_path_segment_plain_id() {
        assert_eq!(encode_path_segment("abc123"), "abc123");
    }

    #[test]
    fn test_encode_path_segment_keeps_unreserved_and_at() {
        assert_eq!(encode_path_segment("1-ab_C.d~e"), "1-ab_C.d~e");
        assert_eq!(encode_path_segment("user@gmail.com"), "user@gmail.com");
        assert_eq!(encode_path_segment("a:b"), "a%3Ab");
    }

    #[test]
    fn test_encode_path_segment_escapes_dot_segments() {
        assert_eq!(encode_path_segment("."), "%2E");
        assert_eq!(encode_path_segment(".."), "%2E%2E");
        assert_eq!(encode_path_segment("..."), "...");
    }

    #[test]
    fn test_encode_path_segment_query_injection() {
        let encoded = encode_path_segment("fileid?fields=name");
        assert!(!encoded.contains('?'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn test_encode_path_segment_fragment_injection() {
        let encoded = encode_path_segment("fileid#section");
        assert!(!encoded.contains('#'));
    }

    #[test]
    fn test_encode_path_segment_path_traversal() {
        let encoded = encode_path_segment("../../etc/passwd");
        assert_eq!(encoded, "..%2F..%2Fetc%2Fpasswd");
    }

    #[test]
    fn test_encode_path_segment_unicode() {
        let encoded = encode_path_segment("日本語ID");
        assert!(!encoded.contains('日'));
    }

    #[test]
    fn test_encode_path_segment_spaces() {
        let encoded = encode_path_segment("my file id");
        assert!(!encoded.contains(' '));
    }

    #[test]
    fn test_encode_path_segment_already_encoded() {
        let encoded = encode_path_segment("user%40gmail.com");
        assert!(encoded.contains("%2540"));
    }

    #[test]
    fn test_encode_path_preserving_slashes_hierarchical_name() {
        let encoded = encode_path_preserving_slashes("projects/p1/locations/us/topics/t1");
        assert_eq!(encoded, "projects/p1/locations/us/topics/t1");
    }

    #[test]
    fn test_encode_path_preserving_slashes_escapes_reserved_chars() {
        let encoded = encode_path_preserving_slashes("hash#1/child?x=y");
        assert_eq!(encoded, "hash%231/child%3Fx%3Dy");
    }

    #[test]
    fn test_encode_path_preserving_slashes_spaces_and_unicode() {
        let encoded = encode_path_preserving_slashes("タイムライン 1/列 A");
        assert!(!encoded.contains(' '));
        assert!(encoded.contains('/'));
    }

    // -- validate_resource_name -----------------------------------------------

    #[test]
    fn test_validate_resource_name_valid() {
        assert!(validate_resource_name("spaces/ABC123").is_ok());
        assert!(validate_resource_name("subscriptions/my-sub").is_ok());
        assert!(validate_resource_name("@default").is_ok());
        assert!(validate_resource_name("projects/p1/topics/t1").is_ok());
    }

    #[test]
    fn test_validate_resource_name_traversal() {
        assert!(validate_resource_name("../../etc/passwd").is_err());
        assert!(validate_resource_name("spaces/../other").is_err());
        assert!(validate_resource_name("..").is_err());
    }

    #[test]
    fn test_validate_resource_name_control_chars() {
        assert!(validate_resource_name("spaces/\0bad").is_err());
        assert!(validate_resource_name("spaces/\nbad").is_err());
        assert!(validate_resource_name("spaces/\rbad").is_err());
        assert!(validate_resource_name("spaces/\tbad").is_err());
    }

    #[test]
    fn test_validate_resource_name_empty() {
        assert!(validate_resource_name("").is_err());
    }

    #[test]
    fn test_validate_resource_name_query_injection() {
        assert!(validate_resource_name("spaces/ABC?key=val").is_err());
        assert!(validate_resource_name("spaces/ABC#fragment").is_err());
    }

    #[test]
    fn test_validate_resource_name_error_messages_are_clear() {
        let err = validate_resource_name("").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));

        let err = validate_resource_name("../bad").unwrap_err();
        assert!(err.to_string().contains("path traversal"));

        let err = validate_resource_name("bad\0id").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }

    #[test]
    fn test_validate_resource_name_percent_bypass() {
        assert!(validate_resource_name("%2e%2e").is_err());
        assert!(validate_resource_name("spaces/%2e%2e/etc").is_err());
        assert!(validate_resource_name("spaces/100%").is_err());
    }

    // --- reject_dangerous_chars Unicode ---

    #[test]
    fn test_reject_dangerous_chars_zero_width_space() {
        assert!(reject_dangerous_chars("foo\u{200B}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_bom() {
        assert!(reject_dangerous_chars("foo\u{FEFF}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_rtl_override() {
        assert!(reject_dangerous_chars("foo\u{202E}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_unicode_line_separator() {
        assert!(reject_dangerous_chars("foo\u{2028}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_paragraph_separator() {
        assert!(reject_dangerous_chars("foo\u{2029}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_zero_width_joiner() {
        assert!(reject_dangerous_chars("foo\u{200D}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_normal_unicode_ok() {
        assert!(reject_dangerous_chars("日本語", "test").is_ok());
        assert!(reject_dangerous_chars("café", "test").is_ok());
        assert!(reject_dangerous_chars("αβγ", "test").is_ok());
    }

    // --- validate_resource_name Unicode ---

    #[test]
    fn test_validate_resource_name_zero_width_chars() {
        assert!(validate_resource_name("foo\u{200B}bar").is_err());
        assert!(validate_resource_name("foo\u{200D}bar").is_err());
        assert!(validate_resource_name("foo\u{FEFF}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_unicode_line_seps() {
        assert!(validate_resource_name("foo\u{2028}bar").is_err());
        assert!(validate_resource_name("foo\u{2029}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_rtl_override() {
        assert!(validate_resource_name("foo\u{202E}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_bidi_embedding() {
        assert!(validate_resource_name("foo\u{202A}bar").is_err());
        assert!(validate_resource_name("foo\u{202B}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_rejects_non_ascii_homoglyphs() {
        assert!(validate_resource_name("spaces/ΑΒС").is_err());
    }

    // SEC-05: characters that can change URL authority or path semantics.
    #[test]
    fn test_validate_resource_name_rejects_authority_chars() {
        for bad in [
            "evil.com@x",
            "projects/p@evil.com",
            "a\\b",
            "spaces\\..\\x",
            "projects/p:443",
            "https://evil",
            "a b",
            "/leading",
            "trailing/",
            "double//slash",
            "./x",
        ] {
            assert!(validate_resource_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_validate_resource_name_allows_known_aliases_and_safe_chars() {
        assert_eq!(validate_resource_name("@default").unwrap(), "@default");
        assert!(validate_resource_name("@me").is_err());
        assert!(validate_resource_name("a.b_c~d-e/F9").is_ok());
    }

    #[test]
    fn test_validate_resource_name_overlong_accepted() {
        let long = "a".repeat(10_000);
        assert!(validate_resource_name(&long).is_ok());
    }

    // --- validate_api_identifier ---

    #[test]
    fn test_validate_api_identifier_valid() {
        assert_eq!(validate_api_identifier("drive").unwrap(), "drive");
        assert_eq!(validate_api_identifier("v3").unwrap(), "v3");
        assert_eq!(
            validate_api_identifier("directory_v1").unwrap(),
            "directory_v1"
        );
        assert_eq!(
            validate_api_identifier("admin.reports_v1").unwrap(),
            "admin.reports_v1"
        );
        assert_eq!(validate_api_identifier("v2beta1").unwrap(), "v2beta1");
    }

    #[test]
    fn test_validate_api_identifier_rejects_path_traversal() {
        assert!(validate_api_identifier("../etc/passwd").is_err());
        assert!(validate_api_identifier("foo/../bar").is_err());
    }

    #[test]
    fn test_validate_api_identifier_rejects_special_chars() {
        assert!(validate_api_identifier("drive?key=val").is_err());
        assert!(validate_api_identifier("drive#frag").is_err());
        assert!(validate_api_identifier("drive%2f..").is_err());
        assert!(validate_api_identifier("v3 ").is_err());
        assert!(validate_api_identifier("v3\n").is_err());
    }

    #[test]
    fn test_validate_api_identifier_empty() {
        assert!(validate_api_identifier("").is_err());
    }

    #[test]
    fn test_validate_api_identifier_rejects_dot_tricks() {
        assert!(validate_api_identifier("..").is_err());
        assert!(validate_api_identifier(".hidden").is_err());
        assert!(validate_api_identifier("a..b").is_err());
        assert!(validate_api_identifier("-v1").is_err());
        assert!(validate_api_identifier(&"a".repeat(MAX_API_IDENTIFIER_LEN + 1)).is_err());
        assert!(validate_api_identifier(&"a".repeat(MAX_API_IDENTIFIER_LEN)).is_ok());
    }
}
