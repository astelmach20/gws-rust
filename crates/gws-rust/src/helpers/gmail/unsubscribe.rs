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
//! * Gmail's `Authentication-Results` reports `dkim=pass`, since RFC 8058
//!   requires the headers to be covered by a valid DKIM signature.
//!
//! The POST carries no credentials or cookies and does not follow redirects.
//! When one-click is not possible the command fails and lists the manual
//! unsubscribe options instead of guessing.

use super::cli::required_str;
use super::prelude::*;
use crate::confirm::{self, Impact};

const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";
const POST_TIMEOUT_SECS: u64 = 30;

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

/// Parse `List-Unsubscribe`: a comma-separated list of `<uri>` entries.
fn parse_list_unsubscribe(value: &str) -> (Vec<String>, Vec<String>) {
    let mut https = Vec::new();
    let mut mailto = Vec::new();
    for entry in value.split(',') {
        let uri = entry
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
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
    info.dkim_pass = header_values(msg, "Authentication-Results")
        .iter()
        .any(|v| {
            let v = v.to_ascii_lowercase();
            v.trim_start().starts_with("mx.google.com") && v.contains("dkim=pass")
        });
    info
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
    let host = parsed.host_str().unwrap_or_default();
    let is_ip = host.starts_with('[') || host.parse::<std::net::IpAddr>().is_ok();
    match host {
        h if !h.is_empty() && !is_ip && !h.eq_ignore_ascii_case("localhost") && h.contains('.') => {
        }
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
        .map_err(|e| GwsError::other(format!("One-click unsubscribe request failed: {e}")))?;
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
    let message_id = required_str(matches, "message-id")?;
    let dry_run = crate::args::dry_run(matches)?;
    // Reading the headers is always a real (read-only) request, so --dry-run can
    // show exactly which URL would receive the one-click POST.
    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let msg = api
        .get_message(
            &message_id,
            "metadata",
            &[
                "From",
                "List-Unsubscribe",
                "List-Unsubscribe-Post",
                "Authentication-Results",
            ],
        )
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

    if dry_run {
        return crate::helpers::http::print_dry_run(
            matches,
            vec![json!({
                "method": "POST",
                "url": target.as_str(),
                "headers": { "Content-Type": "application/x-www-form-urlencoded" },
                "body": ONE_CLICK_BODY,
            })],
        );
    }
    confirm::confirm(
        matches,
        Impact::Outbound,
        &format!("unsubscribe via {}", target.host_str().unwrap_or_default()),
    )?;
    let status = post_one_click(&target).await?;
    let out = json!({
        "unsubscribed": true,
        "messageId": message_id,
        "url": target.as_str(),
        "status": status,
    });
    let format = crate::helpers::http::output_format(matches)?;
    super::emit_screened(sanitize, &format, out).await
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
    fn refuses_unsafe_targets() {
        for url in [
            "http://list.example.com/u",
            "https://127.0.0.1/u",
            "https://[::1]/u",
            "https://localhost/u",
            "https://intranet/u",
            "https://user:pw@list.example.com/u",
        ] {
            assert!(validate_target(url).is_err(), "{url} must be rejected");
        }
        assert!(validate_target("https://list.example.com/u?x=1").is_ok());
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
