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

//! `gmail +filter list|create|delete`: manage Gmail filters.

use super::api::resolve_label_ids;
use super::cli::{list_values, parse_optional_trimmed, required_str};
use super::prelude::*;
use crate::confirm::{self, Impact};

/// A filter to create, with label names not yet resolved to IDs.
#[derive(Debug, Clone, PartialEq)]
struct FilterSpec {
    criteria: serde_json::Map<String, Value>,
    add_labels: Vec<String>,
    remove_labels: Vec<String>,
    forward: Option<String>,
}

fn parse_create_args(matches: &ArgMatches) -> Result<FilterSpec, GwsError> {
    let mut criteria = serde_json::Map::new();
    for (flag, field) in [
        ("from", "from"),
        ("to", "to"),
        ("subject", "subject"),
        ("query", "query"),
        ("exclude", "negatedQuery"),
    ] {
        if let Some(v) = parse_optional_trimmed(matches, flag)? {
            criteria.insert(field.to_string(), json!(v));
        }
    }
    if crate::args::flag(matches, "has-attachment")? {
        criteria.insert("hasAttachment".to_string(), json!(true));
    }
    if criteria.is_empty() {
        return Err(GwsError::Validation(
            "Provide at least one criterion (--from, --to, --subject, --query, --exclude, --has-attachment)".to_string(),
        ));
    }

    let mut add_labels = list_values(matches, "add-label")?;
    let mut remove_labels = list_values(matches, "remove-label")?;
    for (flag, system_label, add) in [
        ("archive", "INBOX", false),
        ("mark-read", "UNREAD", false),
        ("star", "STARRED", true),
        ("important", "IMPORTANT", true),
        ("never-important", "IMPORTANT", false),
        ("trash", "TRASH", true),
        ("never-spam", "SPAM", false),
    ] {
        if crate::args::flag(matches, flag)? {
            if add {
                add_labels.push(system_label.to_string());
            } else {
                remove_labels.push(system_label.to_string());
            }
        }
    }
    let forward = parse_optional_trimmed(matches, "forward")?;
    if add_labels.is_empty() && remove_labels.is_empty() && forward.is_none() {
        return Err(GwsError::Validation(
            "Provide at least one filter action".to_string(),
        ));
    }
    Ok(FilterSpec {
        criteria,
        add_labels,
        remove_labels,
        forward,
    })
}

/// Build the `users.settings.filters` resource from resolved label IDs.
fn build_filter(spec: &FilterSpec, add: &[String], remove: &[String]) -> Value {
    let mut action = serde_json::Map::new();
    if !add.is_empty() {
        action.insert("addLabelIds".to_string(), json!(add));
    }
    if !remove.is_empty() {
        action.insert("removeLabelIds".to_string(), json!(remove));
    }
    if let Some(f) = &spec.forward {
        action.insert("forward".to_string(), json!(f));
    }
    json!({ "criteria": spec.criteria, "action": action })
}

fn print(value: &Value, matches: &ArgMatches) -> Result<(), GwsError> {
    let format = crate::helpers::http::output_format(matches)?;
    crate::output::emit(&crate::formatter::format_value(value, &format)?)?;
    Ok(())
}

/// Handle `+filter`.
pub(super) async fn handle_filter(matches: &ArgMatches) -> Result<(), GwsError> {
    let dry_run = crate::args::dry_run(matches)?;
    let base = format!("{}/users/me/settings/filters", super::api::GMAIL_API_BASE);
    match matches.subcommand() {
        Some(("list", sub)) => {
            if dry_run {
                return crate::helpers::http::print_dry_run(
                    matches,
                    vec![crate::helpers::http::dry_run_request(
                        "GET",
                        &base,
                        &[],
                        None,
                    )],
                );
            }
            let api = super::api::authenticated(&[GMAIL_SETTINGS_SCOPE]).await?;
            let filters = api.list_filters().await?;
            print(&filters, sub)
        }
        Some(("create", sub)) => {
            let spec = parse_create_args(sub)?;
            if let Some(to) = &spec.forward {
                confirm::confirm(
                    sub,
                    Impact::Outbound,
                    &format!("create a filter that forwards matching mail to {to}"),
                )?;
            }
            if dry_run {
                let body = build_filter(&spec, &spec.add_labels, &spec.remove_labels);
                return crate::helpers::http::print_dry_run(
                    matches,
                    vec![crate::helpers::http::dry_run_request(
                        "POST",
                        &base,
                        &[],
                        Some(&body),
                    )],
                );
            }
            let api = super::api::authenticated(&[GMAIL_SETTINGS_SCOPE]).await?;
            let body = if spec.add_labels.is_empty() && spec.remove_labels.is_empty() {
                build_filter(&spec, &[], &[])
            } else {
                let labels = api.list_labels().await?;
                let add = resolve_label_ids(&spec.add_labels, &labels)?;
                let remove = resolve_label_ids(&spec.remove_labels, &labels)?;
                build_filter(&spec, &add, &remove)
            };
            let created = api.create_filter(&body).await?;
            print(&created, sub)
        }
        Some(("delete", sub)) => {
            let id = required_str(sub, "filter-id")?;
            confirm::confirm(
                sub,
                Impact::Destructive,
                &format!("delete Gmail filter {id}"),
            )?;
            if dry_run {
                let url = format!("{base}/{}", crate::validate::encode_path_segment(&id));
                return crate::helpers::http::print_dry_run(
                    matches,
                    vec![crate::helpers::http::dry_run_request(
                        "DELETE",
                        &url,
                        &[],
                        None,
                    )],
                );
            }
            let api = super::api::authenticated(&[GMAIL_SETTINGS_SCOPE]).await?;
            api.delete_filter(&id).await?;
            print(&json!({ "deleted": true, "filterId": id }), sub)
        }
        Some((other, _)) => Err(GwsError::other(format!("unknown +filter action '{other}'"))),
        None => Err(GwsError::Validation(
            "Specify list, create, or delete".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::gmail::test_support::helper_matches;

    fn create(args: &[&str]) -> ArgMatches {
        let mut full = vec!["+filter", "create"];
        full.extend_from_slice(args);
        helper_matches(&full)
            .subcommand_matches("create")
            .unwrap()
            .clone()
    }

    #[test]
    fn create_maps_flags_to_filter_resource() {
        let spec = parse_create_args(&create(&[
            "--from",
            "news@example.com",
            "--exclude",
            "urgent",
            "--has-attachment",
            "--add-label",
            "Newsletters",
            "--archive",
            "--mark-read",
            "--star",
            "--never-spam",
        ]))
        .unwrap();
        assert_eq!(spec.criteria["from"], "news@example.com");
        assert_eq!(spec.criteria["negatedQuery"], "urgent");
        assert_eq!(spec.criteria["hasAttachment"], true);
        assert_eq!(spec.add_labels, vec!["Newsletters", "STARRED"]);
        assert_eq!(spec.remove_labels, vec!["INBOX", "UNREAD", "SPAM"]);

        let body = build_filter(&spec, &["Label_1".into()], &["INBOX".into()]);
        assert_eq!(body["action"]["addLabelIds"], json!(["Label_1"]));
        assert_eq!(body["action"]["removeLabelIds"], json!(["INBOX"]));
        assert!(body["action"].get("forward").is_none());
    }

    #[test]
    fn create_requires_criteria_and_action() {
        let root = crate::commands::build_cli(&crate::discovery::RestDescription {
            name: "gmail".into(),
            ..Default::default()
        });
        assert!(
            root.clone()
                .try_get_matches_from(["gwsr", "+filter", "create", "--archive"])
                .is_err()
        );
        assert!(
            root.try_get_matches_from(["gwsr", "+filter", "create", "--from", "x"])
                .is_err()
        );
    }

    #[test]
    fn forward_only_filter() {
        let spec = parse_create_args(&create(&[
            "--to",
            "me@example.com",
            "--forward",
            "b@example.com",
        ]))
        .unwrap();
        let body = build_filter(&spec, &[], &[]);
        assert_eq!(body["action"], json!({"forward": "b@example.com"}));
    }

    /// SEC-25: deleting a filter is destructive and always needs --yes.
    #[tokio::test]
    async fn delete_without_yes_is_refused_before_any_request() {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            return;
        }
        let m = helper_matches(&["+filter", "delete", "--filter-id", "f1"]);
        let err = handle_filter(&m).await.unwrap_err();
        assert!(err.to_string().contains("Confirmation required"), "{err}");
    }

    #[tokio::test]
    async fn delete_dry_run_needs_no_confirmation() {
        let m = helper_matches(&["+filter", "delete", "--filter-id", "f1", "--dry-run"]);
        handle_filter(&m).await.unwrap();
    }
}
