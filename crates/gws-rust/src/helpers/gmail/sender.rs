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

//! Sender identity resolution via Gmail send-as settings and the People API.

use super::prelude::*;

#[derive(Debug)]
struct SendAsIdentity {
    mailbox: Mailbox,
}

/// Parse the JSON response from `users.settings.sendAs.list` into identities.
///
/// Entries without a `sendAsEmail` cannot be used as a From address and are
/// skipped; a response whose `sendAs` field is not an array is an error.
fn parse_send_as_response(body: &Value) -> Result<Vec<SendAsIdentity>, GwsError> {
    let Some(entries) = body.get("sendAs") else {
        return Ok(Vec::new());
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| GwsError::other("sendAs.list response: 'sendAs' is not an array"))?;

    Ok(entries
        .iter()
        .filter_map(|entry| {
            let email = entry.get("sendAsEmail")?.as_str()?;
            let display_name = entry
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            // Build a formatted address string so Mailbox::parse applies
            // sanitize_control_chars, consistent with all other Mailbox creation paths.
            let raw = match display_name {
                Some(name) => format!("{name} <{email}>"),
                None => email.to_string(),
            };
            Some(SendAsIdentity {
                mailbox: Mailbox::parse(&raw),
            })
        })
        .collect())
}

/// Match each `--from` address against the configured send-as identities.
///
/// Bare emails are enriched with the identity's display name; addresses that
/// already carry a display name keep it. An address that is not a configured
/// send-as identity is an error: Gmail would otherwise silently rewrite the
/// From header to the primary address.
fn resolve_sender_from_identities(
    from: &[Mailbox],
    identities: &[SendAsIdentity],
) -> Result<Vec<Mailbox>, GwsError> {
    from.iter()
        .map(|m| {
            let identity = identities
                .iter()
                .find(|id| id.mailbox.email.eq_ignore_ascii_case(&m.email))
                .ok_or_else(|| {
                    let configured: Vec<&str> = identities
                        .iter()
                        .map(|id| id.mailbox.email.as_str())
                        .collect();
                    GwsError::Validation(format!(
                        "--from {} is not a configured send-as address (configured: {})",
                        m.email,
                        if configured.is_empty() {
                            "none".to_string()
                        } else {
                            configured.join(", ")
                        }
                    ))
                })?;
            Ok(if m.name.is_some() {
                m.clone()
            } else {
                identity.mailbox.clone()
            })
        })
        .collect()
}

/// Resolve the `From` header for an outgoing message.
///
/// Without `--from` the header is left unset and Gmail fills in the account's
/// default send-as identity (including its display name). With `--from`, the
/// addresses are validated against the account's send-as identities.
pub(super) async fn resolve_sender(
    api: &GmailApi,
    from: Option<&[Mailbox]>,
) -> Result<Option<Vec<Mailbox>>, GwsError> {
    let Some(from) = from else {
        return Ok(None);
    };
    let body = api.send_as_list().await?;
    let identities = parse_send_as_response(&body)?;
    resolve_sender_from_identities(from, &identities).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::mock_api;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_parse_send_as_response() {
        let body = serde_json::json!({
            "sendAs": [
                {
                    "sendAsEmail": "malo@intelligence.org",
                    "displayName": "Malo Bourgon",
                    "replyToAddress": "",
                    "signature": "",
                    "isPrimary": true,
                    "isDefault": true,
                    "treatAsAlias": false,
                    "verificationStatus": "accepted"
                },
                {
                    "sendAsEmail": "malo@work.com",
                    "displayName": "Malo (Work)",
                    "replyToAddress": "",
                    "signature": "",
                    "isPrimary": false,
                    "isDefault": false,
                    "treatAsAlias": true,
                    "verificationStatus": "accepted"
                },
                {
                    "sendAsEmail": "noreply@example.com",
                    "displayName": "",
                    "isPrimary": false,
                    "isDefault": false,
                    "verificationStatus": "accepted"
                }
            ]
        });

        let ids = parse_send_as_response(&body).unwrap();
        assert_eq!(ids.len(), 3);

        assert_eq!(ids[0].mailbox.email, "malo@intelligence.org");
        assert_eq!(ids[0].mailbox.name.as_deref(), Some("Malo Bourgon"));

        assert_eq!(ids[1].mailbox.email, "malo@work.com");
        assert_eq!(ids[1].mailbox.name.as_deref(), Some("Malo (Work)"));

        // Empty displayName becomes None
        assert_eq!(ids[2].mailbox.email, "noreply@example.com");
        assert!(ids[2].mailbox.name.is_none());
    }

    #[test]
    fn test_parse_send_as_response_empty() {
        let body = serde_json::json!({});
        let ids = parse_send_as_response(&body).unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn test_parse_send_as_response_skips_missing_email() {
        let body = serde_json::json!({
            "sendAs": [
                { "displayName": "No Email", "isDefault": true },
                { "sendAsEmail": "valid@example.com", "isDefault": false }
            ]
        });
        let ids = parse_send_as_response(&body).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].mailbox.email, "valid@example.com");
    }

    fn make_identities() -> Vec<SendAsIdentity> {
        vec![
            SendAsIdentity {
                mailbox: Mailbox::parse("Malo Bourgon <malo@intelligence.org>"),
            },
            SendAsIdentity {
                mailbox: Mailbox::parse("Malo (Work) <malo@work.com>"),
            },
            SendAsIdentity {
                mailbox: Mailbox::parse("bare@example.com"),
            },
        ]
    }

    #[test]
    fn test_parse_send_as_response_rejects_non_array() {
        assert!(parse_send_as_response(&json!({"sendAs": "x"})).is_err());
    }

    #[test]
    fn test_resolve_sender_bare_email_enriched() {
        let from = [Mailbox::parse("malo@work.com")];
        let addrs = resolve_sender_from_identities(&from, &make_identities()).unwrap();
        assert_eq!(addrs[0].email, "malo@work.com");
        assert_eq!(addrs[0].name.as_deref(), Some("Malo (Work)"));
    }

    #[test]
    fn test_resolve_sender_bare_email_case_insensitive() {
        let from = [Mailbox::parse("Malo@Work.Com")];
        let addrs = resolve_sender_from_identities(&from, &make_identities()).unwrap();
        assert_eq!(addrs[0].name.as_deref(), Some("Malo (Work)"));
    }

    #[test]
    fn test_resolve_sender_unknown_address_is_an_error() {
        let from = [Mailbox::parse("unknown@example.com")];
        let err = resolve_sender_from_identities(&from, &make_identities()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown@example.com"));
        assert!(msg.contains("malo@work.com"));
    }

    #[test]
    fn test_resolve_sender_with_display_name_returns_as_is() {
        let from = [Mailbox::parse("Custom Name <malo@work.com>")];
        let addrs = resolve_sender_from_identities(&from, &make_identities()).unwrap();
        assert_eq!(addrs[0].name.as_deref(), Some("Custom Name"));
    }

    #[test]
    fn test_resolve_sender_identity_without_name_stays_bare() {
        let from = [Mailbox::parse("bare@example.com")];
        let addrs = resolve_sender_from_identities(&from, &make_identities()).unwrap();
        assert!(addrs[0].name.is_none());
    }

    #[tokio::test]
    async fn test_resolve_sender_without_from_makes_no_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        assert!(
            resolve_sender(&mock_api(&server), None)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_resolve_sender_propagates_send_as_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/settings/sendAs"))
            .respond_with(ResponseTemplate::new(403).set_body_json(
                json!({"error": {"code": 403, "message": "Insufficient Permission"}}),
            ))
            .mount(&server)
            .await;
        let from = [Mailbox::parse("a@b.c")];
        let err = resolve_sender(&mock_api(&server), Some(&from))
            .await
            .unwrap_err();
        assert!(matches!(err, GwsError::Api { code: 403, .. }));
    }
}
