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

//! Resolve Gmail web URLs to API IDs.
//!
//! The Gmail web UI shows either legacy hex IDs (`#inbox/18f1a2b3c4d5e6f7`,
//! identical to the API ID) or opaque tokens such as `FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW`.
//! The opaque tokens are a base conversion: the token is a number written in a
//! 40-character consonant alphabet; re-expressed in base64 it decodes to text
//! such as `f:1864099454408550099` (or `thread-f:…` / `msg-f:…`), where the
//! decimal number is the API ID. Tokens of other kinds (e.g. `thread-a:r…` for
//! unsent drafts) have no API ID and are reported as errors.

use super::cli::required_str;
use super::prelude::*;

/// The consonant alphabet used by Gmail web URL tokens.
const TOKEN_ALPHABET: &[u8] = b"BCDFGHJKLMNPQRSTVWXZbcdfghjklmnpqrstvwxz";
/// Standard base64 alphabet.
const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// What a resolved ID refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum IdKind {
    Thread,
    Message,
}

/// A resolved Gmail API ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ResolvedId {
    pub kind: IdKind,
    /// Hex API ID usable with `users.threads` / `users.messages`.
    pub id: String,
    /// The token or hex ID found in the input.
    pub source: String,
}

/// Re-express a number written in `from` digits as `to` digits (big-integer base conversion).
fn convert_base(input: &[u8], from: &[u8], to: &[u8]) -> Result<String, GwsError> {
    let from_base = from.len() as u32;
    let to_base = to.len() as u32;
    // Little-endian digits in the target base.
    let mut digits: Vec<u32> = Vec::new();
    for &ch in input {
        let value = from.iter().position(|&c| c == ch).ok_or_else(|| {
            GwsError::Validation(format!(
                "'{}' is not a valid Gmail URL token character",
                ch as char
            ))
        })?;
        let mut carry = value as u32;
        for d in digits.iter_mut() {
            let v = *d * from_base + carry;
            *d = v % to_base;
            carry = v / to_base;
        }
        while carry > 0 {
            digits.push(carry % to_base);
            carry /= to_base;
        }
    }
    Ok(digits
        .iter()
        .rev()
        .map(|&d| to[d as usize] as char)
        .collect())
}

/// Decode an opaque Gmail web token (e.g. `FMfcgz…`) to its API ID.
pub(super) fn decode_web_token(token: &str) -> Result<ResolvedId, GwsError> {
    let b64 = convert_base(token.as_bytes(), TOKEN_ALPHABET, BASE64_ALPHABET)?;
    let padded = format!("{b64}{}", "=".repeat((4 - b64.len() % 4) % 4));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(padded)
        .map_err(|e| {
            GwsError::Validation(format!("Gmail URL token '{token}' did not decode: {e}"))
        })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        GwsError::Validation(format!("Gmail URL token '{token}' did not decode to text"))
    })?;
    let (kind, number) = if let Some(n) = text
        .strip_prefix("thread-f:")
        .or_else(|| text.strip_prefix("f:"))
    {
        (IdKind::Thread, n)
    } else if let Some(n) = text.strip_prefix("msg-f:") {
        (IdKind::Message, n)
    } else {
        return Err(GwsError::Validation(format!(
            "Gmail URL token '{token}' refers to '{}', which has no API ID (e.g. an unsent draft)",
            sanitize_for_terminal(&text)
        )));
    };
    let value: u64 = number.parse().map_err(|_| {
        GwsError::Validation(format!(
            "Gmail URL token '{token}' decoded to a non-numeric ID"
        ))
    })?;
    Ok(ResolvedId {
        kind,
        id: format!("{value:x}"),
        source: token.to_string(),
    })
}

fn is_hex_id(s: &str) -> bool {
    (8..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_web_token(s: &str) -> bool {
    s.len() >= 16 && s.bytes().all(|b| TOKEN_ALPHABET.contains(&b))
}

/// Resolve a Gmail web URL, a bare web token, or a hex ID to an API ID.
///
/// The ID is taken from the last path segment of the URL fragment
/// (`#inbox/ID`, `#label/Work/ID`, `#search/query/ID`), ignoring any query.
pub(super) fn resolve(input: &str) -> Result<ResolvedId, GwsError> {
    let input = input.trim();
    let candidate = match input.split_once('#') {
        Some((_, fragment)) => fragment
            .split('?')
            .next()
            .unwrap_or_default()
            .rsplit('/')
            .next()
            .unwrap_or_default(),
        None if input.contains("://") => {
            return Err(GwsError::Validation(format!(
                "'{}' has no '#...' fragment; copy the URL of an open conversation",
                sanitize_for_terminal(input)
            )));
        }
        None => input,
    };
    if is_hex_id(candidate) {
        return Ok(ResolvedId {
            kind: IdKind::Thread,
            id: candidate.to_ascii_lowercase(),
            source: candidate.to_string(),
        });
    }
    if is_web_token(candidate) {
        return decode_web_token(candidate);
    }
    Err(GwsError::Validation(format!(
        "Could not find a Gmail thread or message ID in '{}'",
        sanitize_for_terminal(input)
    )))
}

/// Resolve a `--thread-id` value that may be a Gmail web URL. Plain API IDs
/// pass through unchanged; URLs must resolve to a thread.
pub(super) fn thread_id_from_input(input: &str) -> Result<String, GwsError> {
    if !input.contains('#') && !input.contains("://") && !is_web_token(input) {
        return Ok(input.to_string());
    }
    let resolved = resolve(input)?;
    match resolved.kind {
        IdKind::Thread => Ok(resolved.id),
        IdKind::Message => Err(GwsError::Validation(format!(
            "'{input}' identifies a message, not a thread; use --message-id {}",
            resolved.id
        ))),
    }
}

/// Handle the `+resolve-url` subcommand.
pub(super) async fn handle_resolve_url(matches: &ArgMatches) -> Result<(), GwsError> {
    let resolved = resolve(&required_str(matches, "url")?)?;
    let verify = !crate::args::flag(matches, "no-verify")? && !crate::args::dry_run(matches)?;
    let mut output = serde_json::to_value(&resolved)
        .map_err(|e| GwsError::other(format!("Failed to serialize result: {e}")))?;
    if verify {
        let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
        let found = match resolved.kind {
            IdKind::Thread => api.get_thread_minimal(&resolved.id).await?,
            IdKind::Message => api.get_message(&resolved.id, "minimal", &[]).await?,
        };
        output["verified"] = json!(true);
        if let Some(thread_id) = found.get("threadId").or_else(|| found.get("id")) {
            output["threadId"] = thread_id.clone();
        }
    } else {
        output["verified"] = json!(false);
    }
    let format = crate::helpers::http::output_format(matches)?;
    crate::output::emit(&crate::formatter::format_value(&output, &format)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inverse of `decode_web_token`, for round-trip tests.
    fn encode_web_token(text: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(text);
        let b64 = b64.trim_end_matches('=');
        convert_base(b64.as_bytes(), BASE64_ALPHABET, TOKEN_ALPHABET).unwrap()
    }

    #[test]
    fn decodes_real_gmail_token() {
        // Token from a real Gmail URL (googleworkspace/cli#790).
        let r = decode_web_token("FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW").unwrap();
        assert_eq!(r.kind, IdKind::Thread);
        assert_eq!(r.id, "19de9c93ce0566d3");
    }

    #[test]
    fn round_trips_thread_and_message_tokens() {
        let t = encode_web_token("thread-f:1618047512345678901");
        let r = decode_web_token(&t).unwrap();
        assert_eq!(r.kind, IdKind::Thread);
        assert_eq!(r.id, format!("{:x}", 1618047512345678901u64));
        let m = encode_web_token("msg-f:1618047512345678902");
        assert_eq!(decode_web_token(&m).unwrap().kind, IdKind::Message);
    }

    #[test]
    fn draft_tokens_are_rejected() {
        let t = encode_web_token("thread-a:r-123456789");
        let err = decode_web_token(&t).unwrap_err();
        assert!(err.to_string().contains("no API ID"));
    }

    #[test]
    fn resolves_urls_and_bare_ids() {
        let r = resolve("https://mail.google.com/mail/u/0/#inbox/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW")
            .unwrap();
        assert_eq!(r.id, "19de9c93ce0566d3");
        let r =
            resolve("https://mail.google.com/mail/u/1/#label/Work+Stuff/18F1A2B3C4D5E6F7").unwrap();
        assert_eq!(r.id, "18f1a2b3c4d5e6f7");
        let r = resolve("https://mail.google.com/mail/u/0/#search/foo/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW?compose=new").unwrap();
        assert_eq!(r.id, "19de9c93ce0566d3");
        assert_eq!(
            resolve("FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW").unwrap().id,
            "19de9c93ce0566d3"
        );
        assert_eq!(resolve("18f1a2b3c4d5e6f7").unwrap().id, "18f1a2b3c4d5e6f7");
    }

    #[test]
    fn rejects_unresolvable_input() {
        assert!(resolve("https://mail.google.com/mail/u/0/").is_err());
        assert!(resolve("https://mail.google.com/mail/u/0/#inbox").is_err());
        assert!(resolve("not an id!").is_err());
    }

    #[test]
    fn thread_id_input_passes_api_ids_through() {
        assert_eq!(
            thread_id_from_input("18f1a2b3c4d5e6f7").unwrap(),
            "18f1a2b3c4d5e6f7"
        );
        assert_eq!(
            thread_id_from_input(
                "https://mail.google.com/mail/u/0/#inbox/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW"
            )
            .unwrap(),
            "19de9c93ce0566d3"
        );
        let msg_token = encode_web_token("msg-f:1618047512345678902");
        assert!(thread_id_from_input(&msg_token).is_err());
    }
}
