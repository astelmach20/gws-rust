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

//! `gmail +unsubscribe`: RFC 8058 one-click unsubscribe.
//!
//! The one-click POST is sent only when all of these hold:
//! * `List-Unsubscribe-Post: List-Unsubscribe=One-Click` is present (RFC 8058 §3.1);
//! * `List-Unsubscribe` contains an `https:` URI whose host is a DNS name
//!   (not an IP literal or localhost);
//! * Gmail's own (topmost `mx.google.com`) `Authentication-Results` header
//!   reports `dkim=pass`, since RFC 8058 requires the headers to be covered
//!   by a valid DKIM signature. Lower headers come from the sender.
//!
//! The POST carries no credentials or cookies and does not follow redirects.
//! When one-click is not possible the command fails and lists the manual
//! unsubscribe options instead of guessing.

use super::cli::required_message_id;
use super::prelude::*;
use crate::confirm::{self, Impact};

const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";
const POST_TIMEOUT_SECS: u64 = 30;
/// Headers read from the message to decide on one-click unsubscribe.
const METADATA_HEADERS: &[&str] = &[
    "From",
    "List-Unsubscribe",
    "List-Unsubscribe-Post",
    "Authentication-Results",
];

/// Unsubscribe-related information extracted from a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct UnsubscribeInfo {
    https: Vec<String>,
    mailto: Vec<String>,
    one_click: bool,
    dkim_pass: bool,
}

/// All values of a header (case-insensitive) in a message resource.
fn header_values<'a>(msg: &'a Value, name: &str) -> Vec<&'a str> {
    msg.get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(Value::as_array)
        .map(|hs| {
            hs.iter()
                .filter(|h| {
                    h.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
                .filter_map(|h| h.get("value").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

/// The URIs of a `List-Unsubscribe` value: the `<...>` entries (RFC 2369
/// §2), in order. A URI may itself contain a comma (`?ids=1,2`), so the
/// value is not split on commas; text outside the angle brackets, such as a
/// `(comment)`, is not part of any URI; and white space inside the brackets
/// (left by header folding) is ignored. A value with no angle brackets at
/// all (non-conforming senders) is split on commas instead.
fn list_unsubscribe_uris(value: &str) -> Vec<String> {
    let without_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if !value.contains('<') {
        return value
            .split(',')
            .map(without_ws)
            .filter(|u| !u.is_empty())
            .collect();
    }
    let mut uris = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            break;
        };
        let uri = without_ws(&after[..close]);
        if !uri.is_empty() {
            uris.push(uri);
        }
        rest = &after[close + 1..];
    }
    uris
}

/// Parse `List-Unsubscribe` into its `https:` and `mailto:` URIs.
fn parse_list_unsubscribe(value: &str) -> (Vec<String>, Vec<String>) {
    let mut https = Vec::new();
    let mut mailto = Vec::new();
    for uri in list_unsubscribe_uris(value) {
        let uri = uri.as_str();
        let lower = uri.to_ascii_lowercase();
        if lower.starts_with("https://") {
            https.push(uri.to_string());
        } else if lower.starts_with("mailto:") {
            mailto.push(uri.to_string());
        }
    }
    (https, mailto)
}

fn extract_info(msg: &Value) -> UnsubscribeInfo {
    let mut info = UnsubscribeInfo::default();
    for v in header_values(msg, "List-Unsubscribe") {
        let (h, m) = parse_list_unsubscribe(v);
        info.https.extend(h);
        info.mailto.extend(m);
    }
    info.one_click = header_values(msg, "List-Unsubscribe-Post")
        .iter()
        .any(|v| v.trim().eq_ignore_ascii_case(ONE_CLICK_BODY));
    info.dkim_pass = gmail_dkim_pass(msg);
    info
}

/// Split an `Authentication-Results` value on the `;` separators of RFC 8601,
/// dropping `(comments)` and ignoring `;` inside comments and quoted strings.
fn auth_results_parts(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' && (quoted || depth > 0) {
            // A quoted-pair: the next character is literal.
            let next = chars.next();
            if depth == 0 {
                current.push(c);
                current.extend(next);
            }
            continue;
        }
        match c {
            '(' if !quoted => {
                depth += 1;
                current.push(' ');
            }
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// Whether Gmail's own `Authentication-Results` header reports `dkim=pass`.
///
/// Only the topmost header whose authserv-id is `mx.google.com` counts: Gmail
/// prepends it, so any header below it (even one claiming to be from
/// `mx.google.com`) was supplied by the sender (RFC 8601 §5). The Gmail API
/// returns `payload.headers` in message order, with or without
/// `format=metadata`.
fn gmail_dkim_pass(msg: &Value) -> bool {
    let Some(parts) = header_values(msg, "Authentication-Results")
        .into_iter()
        .map(auth_results_parts)
        .find(|parts| {
            parts
                .first()
                .and_then(|id| id.split_whitespace().next())
                .is_some_and(|id| id.eq_ignore_ascii_case("mx.google.com"))
        })
    else {
        return false;
    };
    parts.iter().skip(1).any(|resinfo| {
        resinfo
            .split_whitespace()
            .next()
            .is_some_and(|result| result.eq_ignore_ascii_case("dkim=pass"))
    })
}

/// Validate a one-click target URL: https and a DNS host name.
fn validate_target(url: &str) -> Result<reqwest::Url, GwsError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| GwsError::Validation(format!("Invalid List-Unsubscribe URL '{url}': {e}")))?;
    if parsed.scheme() != "https" {
        return Err(GwsError::Validation(format!(
            "List-Unsubscribe URL '{url}' is not https"
        )));
    }
    // A trailing dot names the same (fully qualified) host: `localhost.` is
    // loopback and `intranet.` a single-label name. RFC 6761 reserves every
    // `*.localhost` name for loopback too.
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let is_ip = host.starts_with('[') || host.parse::<std::net::IpAddr>().is_ok();
    let loopback_name = host == "localhost" || host.ends_with(".localhost");
    match host.as_str() {
        h if !h.is_empty() && !is_ip && !loopback_name && h.contains('.') => {}
        _ => {
            return Err(GwsError::Validation(format!(
                "List-Unsubscribe URL '{url}' must use a public DNS host name"
            )));
        }
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(GwsError::Validation(format!(
            "List-Unsubscribe URL '{url}' must not contain credentials"
        )));
    }
    Ok(parsed)
}

/// Pick the one-click target, or explain why one-click is unavailable.
fn one_click_target(info: &UnsubscribeInfo) -> Result<reqwest::Url, String> {
    if info.https.is_empty() && info.mailto.is_empty() {
        return Err("the message has no List-Unsubscribe header".to_string());
    }
    if !info.one_click {
        return Err(
            "the sender does not advertise RFC 8058 one-click (List-Unsubscribe-Post)".to_string(),
        );
    }
    if !info.dkim_pass {
        return Err("Gmail did not report a passing DKIM signature for this message".to_string());
    }
    let mut last_err = "no https List-Unsubscribe URL".to_string();
    for url in &info.https {
        match validate_target(url) {
            Ok(u) => return Ok(u),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// Send the RFC 8058 POST. No credentials, no cookies, no redirects.
async fn post_one_click(url: &reqwest::Url) -> Result<u16, GwsError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(POST_TIMEOUT_SECS))
        .build()
        .map_err(|e| GwsError::other(format!("Failed to build HTTP client: {e}")))?;
    let resp = client
        .post(url.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(ONE_CLICK_BODY)
        .send()
        .await
        // No response (refused, DNS, TLS, timeout) is a network error.
        // reqwest's message names the full URL, whose path and query often
        // carry a per-recipient token: name the host only.
        .map_err(|e| {
            GwsError::Network(
                format!(
                    "One-click unsubscribe request to {} failed: {}",
                    url.host_str().unwrap_or_default(),
                    e.without_url()
                )
                .into(),
            )
        })?;
    let status = resp.status();
    if status.is_success() {
        Ok(status.as_u16())
    } else {
        Err(GwsError::Api {
            code: status.as_u16(),
            message: format!(
                "One-click unsubscribe to {} returned HTTP {status}",
                url.host_str().unwrap_or_default()
            ),
            reason: "unsubscribeFailed".to_string(),
            enable_url: None,
        })
    }
}

/// Handle `+unsubscribe`.
pub(super) async fn handle_unsubscribe(
    matches: &ArgMatches,
    sanitize: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let message_id = required_message_id(matches)?;
    if crate::args::dry_run(matches)? {
        // --dry-run loads no credentials and sends nothing, so the one-click
        // URL (which comes from the message's headers) is shown as a placeholder.
        let api = GmailApi::new(crate::transport::Transport::unauthenticated()?);
        let query: Vec<(&str, String)> = std::iter::once(("format", "metadata".to_string()))
            .chain(
                METADATA_HEADERS
                    .iter()
                    .map(|h| ("metadataHeaders", (*h).to_string())),
            )
            .collect();
        return crate::helpers::http::print_dry_run(
            matches,
            vec![
                crate::helpers::http::dry_run_request(
                    "GET",
                    &api.message_url(&message_id),
                    &query,
                    None,
                ),
                json!({
                    "method": "POST",
                    "url": "<https URL from the message's List-Unsubscribe header>",
                    "headers": { "Content-Type": "application/x-www-form-urlencoded" },
                    "body": ONE_CLICK_BODY,
                    "note": "sent only if the message advertises one-click unsubscribe and Gmail reports dkim=pass",
                }),
            ],
        );
    }
    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let msg = api
        .get_message(&message_id, "metadata", METADATA_HEADERS)
        .await?;
    let info = extract_info(&msg);
    let target = one_click_target(&info).map_err(|reason| {
        let mut manual: Vec<String> = info.https.clone();
        manual.extend(info.mailto.iter().cloned());
        GwsError::Validation(format!(
            "One-click unsubscribe is not available: {reason}.{}",
            if manual.is_empty() {
                String::new()
            } else {
                format!(
                    " Manual options: {}",
                    sanitize_for_terminal(&manual.join(", "))
                )
            }
        ))
    })?;

    confirm::confirm(
        matches,
        Impact::Outbound,
        &format!("unsubscribe via {}", target.host_str().unwrap_or_default()),
    )?;
    // The target URL comes from the (untrusted) message: screen it before the
    // POST, since screening the result afterwards could not undo it.
    let screening = crate::helpers::modelarmor::screen_outgoing(
        sanitize,
        &json!({ "messageId": message_id, "url": target.as_str() }),
        None,
    )
    .await?;
    let status = post_one_click(&target).await?;
    let mut out = json!({
        "unsubscribed": true,
        "messageId": message_id,
        "url": target.as_str(),
        "status": status,
    });
    if let Some(annotation) = screening {
        out = crate::helpers::modelarmor::annotate(out, annotation);
    }
    let format = crate::helpers::http::output_format(matches)?;
    crate::output::emit(&crate::formatter::format_value(&out, &format)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn msg(headers: &[(&str, &str)]) -> Value {
        let hs: Vec<Value> = headers
            .iter()
            .map(|(n, v)| json!({"name": n, "value": v}))
            .collect();
        json!({ "payload": { "headers": hs } })
    }

    fn good() -> Value {
        msg(&[
            (
                "List-Unsubscribe",
                "<mailto:u@list.example.com?subject=unsub>, <https://list.example.com/u/abc>",
            ),
            ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
            (
                "Authentication-Results",
                "mx.google.com; dkim=pass header.i=@example.com; spf=pass",
            ),
        ])
    }

    #[test]
    fn extracts_uris_and_flags() {
        let info = extract_info(&good());
        assert_eq!(info.https, vec!["https://list.example.com/u/abc"]);
        assert_eq!(info.mailto, vec!["mailto:u@list.example.com?subject=unsub"]);
        assert!(info.one_click);
        assert!(info.dkim_pass);
        assert_eq!(
            one_click_target(&info).unwrap().as_str(),
            "https://list.example.com/u/abc"
        );
    }

    #[test]
    fn list_unsubscribe_uris_are_the_angle_bracket_entries() {
        // A comma inside a URI does not split it, and a comment after an
        // entry (RFC 2369 §2) is not part of the URI.
        let (https, mailto) = parse_list_unsubscribe(
            "<https://list.example.com/u?ids=1,2&t=x> (one click), \
             <mailto:u@list.example.com?subject=a,b> (Use this to leave)",
        );
        assert_eq!(https, vec!["https://list.example.com/u?ids=1,2&t=x"]);
        assert_eq!(mailto, vec!["mailto:u@list.example.com?subject=a,b"]);
        let info = extract_info(&msg(&[
            (
                "List-Unsubscribe",
                "<https://list.example.com/u?ids=1,2> (unsubscribe)",
            ),
            ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
            ("Authentication-Results", "mx.google.com; dkim=pass"),
        ]));
        assert_eq!(
            one_click_target(&info).unwrap().as_str(),
            "https://list.example.com/u?ids=1,2"
        );

        // White space a folded header leaves inside a URI is ignored.
        let (https, _) = parse_list_unsubscribe("<https://list.example.com/\r\n u/abc>");
        assert_eq!(https, vec!["https://list.example.com/u/abc"]);

        // Without angle brackets (non-conforming), entries are split on commas.
        let (https, mailto) =
            parse_list_unsubscribe("https://list.example.com/u, mailto:u@list.example.com");
        assert_eq!(https, vec!["https://list.example.com/u"]);
        assert_eq!(mailto, vec!["mailto:u@list.example.com"]);
    }

    #[test]
    fn refuses_without_one_click_header() {
        let info = extract_info(&msg(&[
            ("List-Unsubscribe", "<https://list.example.com/u>"),
            ("Authentication-Results", "mx.google.com; dkim=pass"),
        ]));
        assert!(one_click_target(&info).unwrap_err().contains("one-click"));
    }

    #[test]
    fn refuses_without_dkim_pass() {
        let info = extract_info(&msg(&[
            ("List-Unsubscribe", "<https://list.example.com/u>"),
            ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
            ("Authentication-Results", "mx.google.com; dkim=fail"),
            // A forged header from another server must not count.
            ("Authentication-Results", "evil.example; dkim=pass"),
        ]));
        assert!(one_click_target(&info).unwrap_err().contains("DKIM"));
    }

    #[test]
    fn only_gmails_topmost_authentication_results_counts() {
        // Gmail prepends its own header; anything below it came from the
        // sender (RFC 8601 §5), even when it claims to be mx.google.com.
        let info = extract_info(&msg(&[
            ("List-Unsubscribe", "<https://list.example.com/u>"),
            ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
            (
                "Authentication-Results",
                "mx.google.com; dkim=fail header.i=@example.com; spf=pass",
            ),
            (
                "Authentication-Results",
                "mx.google.com; dkim=pass header.i=@example.com",
            ),
        ]));
        assert!(!info.dkim_pass);
        assert!(one_click_target(&info).unwrap_err().contains("DKIM"));
    }

    #[test]
    fn dkim_pass_must_be_a_dkim_result_from_gmail() {
        for value in [
            // Another server whose name merely starts with mx.google.com.
            "mx.google.com.evil.example; dkim=pass",
            // `dkim=pass` inside a comment or another method's value.
            "mx.google.com; dkim=fail; spf=pass (google.com: domain of dkim=pass@example.com) smtp.mailfrom=dkim=pass@example.com",
            "mx.google.com; dkim=passed",
            "mx.google.com; arc=pass (i=1 dkim=pass)",
            "mx.google.com; spf=pass smtp.mailfrom=\"a;dkim=pass\"@example.com",
        ] {
            let info = extract_info(&msg(&[("Authentication-Results", value)]));
            assert!(!info.dkim_pass, "{value}");
        }
        for value in [
            "mx.google.com; dkim=pass header.i=@example.com",
            "MX.Google.Com;\r\n       dkim=neutral (no key) header.d=a.example;\r\n       dkim=pass (test mode) header.d=example.com",
            "mx.google.com;\r\n       dkim=pass",
        ] {
            let info = extract_info(&msg(&[("Authentication-Results", value)]));
            assert!(info.dkim_pass, "{value}");
        }
    }

    #[test]
    fn refuses_unsafe_targets() {
        for url in [
            "http://list.example.com/u",
            "https://127.0.0.1/u",
            "https://[::1]/u",
            "https://localhost/u",
            "https://localhost./u",
            "https://LocalHost.:8443/u",
            "https://admin.localhost/u",
            "https://intranet/u",
            "https://intranet./u",
            "https://user:pw@list.example.com/u",
        ] {
            assert!(validate_target(url).is_err(), "{url} must be rejected");
        }
        assert!(validate_target("https://list.example.com/u?x=1").is_ok());
        assert!(validate_target("https://list.example.com./u").is_ok());
    }

    #[test]
    fn no_header_is_reported() {
        assert!(
            one_click_target(&extract_info(&msg(&[])))
                .unwrap_err()
                .contains("no List-Unsubscribe")
        );
    }

    #[tokio::test]
    async fn post_sends_rfc8058_body_and_reports_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/u"))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(body_string(ONE_CLICK_BODY))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;
        let url = reqwest::Url::parse(&format!("{}/u", server.uri())).unwrap();
        assert_eq!(post_one_click(&url).await.unwrap(), 202);
    }

    /// No response from the sender's server is a network error (exit 10),
    /// not an internal one (5).
    #[tokio::test]
    async fn post_without_a_response_is_a_network_error() {
        // Nothing listens on the discard port: the connection is refused.
        let url = reqwest::Url::parse("http://127.0.0.1:9/u").unwrap();
        let err = post_one_click(&url).await.unwrap_err();
        assert!(matches!(err, GwsError::Network(_)), "{err:?}");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_NETWORK, "{err:?}");
        assert!(err.to_string().contains("One-click unsubscribe"), "{err}");
    }

    /// The URL's path and query often carry a per-recipient token: a
    /// transport error names the host only, like the main client's errors.
    #[tokio::test]
    async fn post_error_does_not_include_the_url_path_or_query() {
        let url = reqwest::Url::parse("http://127.0.0.1:9/u/PATHTOKEN?token=QUERYTOKEN").unwrap();
        let err = post_one_click(&url).await.unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, GwsError::Network(_)), "{err:?}");
        assert!(message.contains("127.0.0.1"), "{message}");
        assert!(!message.contains("PATHTOKEN"), "{message}");
        assert!(!message.contains("QUERYTOKEN"), "{message}");
    }

    #[tokio::test]
    async fn post_does_not_follow_redirects() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/u"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/elsewhere"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let url = reqwest::Url::parse(&format!("{}/u", server.uri())).unwrap();
        let err = post_one_click(&url).await.unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 302, .. }));
    }
}
