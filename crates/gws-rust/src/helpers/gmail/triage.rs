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

//! `gmail +triage`: compact summary of unread (or matching) messages.

use super::cli::{required_str, value_or_default};
use super::prelude::*;
use super::search::{SearchParams, header, search};
use crate::helpers::modelarmor::{SanitizeConfig, require_pass, sanitize_value};

/// Handle the `+triage` subcommand.
pub(super) async fn handle_triage(
    matches: &ArgMatches,
    sanitize_config: &SanitizeConfig,
) -> Result<(), GwsError> {
    let params = SearchParams {
        query: Some(required_str(matches, "query")?),
        max: value_or_default::<u32>(matches, "max")?,
        page_token: None,
        include_spam_trash: false,
    };
    let show_labels = matches.get_flag("labels");
    let format =
        crate::helpers::rest::output_format(matches, crate::formatter::OutputFormat::Json)?;
    if crate::helpers::rest::dry_run(matches)? {
        return super::search::dry_run_list(&params);
    }

    // gmail.readonly (not gmail.metadata) because the metadata scope rejects `q`.
    let api = super::api::authenticated(&[GMAIL_READONLY_SCOPE]).await?;
    let results = search(&api, &params).await?;
    let query = params.query.as_deref().unwrap_or_default();
    if results.messages.is_empty() {
        eprintln!("{}", no_messages_msg(query));
    }
    let output = triage_output(
        query,
        &results.messages,
        results.result_size_estimate,
        show_labels,
    );
    let output = require_pass(sanitize_value(sanitize_config, output).await?)?;
    println!("{}", crate::formatter::format_value(&output, &format));
    Ok(())
}

/// Build the triage output document.
fn triage_output(
    query: &str,
    messages: &[Value],
    estimate: Option<u64>,
    show_labels: bool,
) -> Value {
    let rows: Vec<Value> = messages
        .iter()
        .map(|m| {
            let mut row = json!({
                "id": m.get("id").cloned().unwrap_or(Value::Null),
                "from": header(m, "From"),
                "subject": header(m, "Subject"),
                "date": header(m, "Date"),
            });
            if show_labels {
                row["labels"] = m.get("labelIds").cloned().unwrap_or_else(|| json!([]));
            }
            row
        })
        .collect();
    json!({
        "messages": rows,
        "resultSizeEstimate": estimate.unwrap_or(messages.len() as u64),
        "query": query,
    })
}

/// Human-readable "no messages" diagnostic (stderr, never stdout).
fn no_messages_msg(query: &str) -> String {
    format!(
        "No messages found matching query: {}",
        sanitize_for_terminal(query)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::helper_matches;

    fn msg() -> Value {
        json!({
            "id": "m1",
            "labelIds": ["INBOX", "UNREAD"],
            "payload": { "headers": [
                { "name": "From", "value": "a@example.com" },
                { "name": "Subject", "value": "Hello" },
                { "name": "Date", "value": "Mon, 1 Jan 2026 00:00:00 +0000" }
            ]}
        })
    }

    #[test]
    fn triage_defaults() {
        let m = helper_matches(&["+triage"]);
        assert_eq!(value_or_default::<u32>(&m, "max").unwrap(), 20);
        assert_eq!(required_str(&m, "query").unwrap(), "is:unread");
        assert!(!m.get_flag("labels"));
    }

    #[test]
    fn triage_empty_output_is_still_json() {
        let out = triage_output("is:unread", &[], Some(0), false);
        let printed = crate::formatter::format_value(&out, &crate::formatter::OutputFormat::Json);
        let parsed: Value = serde_json::from_str(&printed).unwrap();
        assert_eq!(parsed["messages"], json!([]));
    }

    #[test]
    fn triage_unknown_format_is_an_error() {
        let m = helper_matches(&["+triage", "--format", "xml"]);
        assert!(
            crate::helpers::rest::output_format(&m, crate::formatter::OutputFormat::Table).is_err()
        );
    }

    #[test]
    fn triage_output_shape() {
        let out = triage_output("is:unread", &[msg()], Some(7), false);
        assert_eq!(out["messages"][0]["from"], "a@example.com");
        assert_eq!(out["messages"][0]["subject"], "Hello");
        assert!(out["messages"][0].get("labels").is_none());
        assert_eq!(out["resultSizeEstimate"], 7);
        let out = triage_output("q", &[msg()], None, true);
        assert_eq!(out["messages"][0]["labels"][1], "UNREAD");
        assert_eq!(out["resultSizeEstimate"], 1);
    }

    #[test]
    fn empty_result_message_is_not_json() {
        let msg = no_messages_msg("label:inbox");
        assert!(serde_json::from_str::<Value>(&msg).is_err());
    }
}
