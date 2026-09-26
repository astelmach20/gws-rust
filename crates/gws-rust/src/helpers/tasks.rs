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

//! Google Tasks helpers: `+add`, `+list`, `+lists`.
//!
//! The Tasks API stores only the *date* of a due time: any time of day is
//! silently discarded by Google (upstream #696). `--due` therefore accepts a
//! date and rejects a non-midnight time instead of losing it.

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, optional, required};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_TASKS: &str = "https://www.googleapis.com/auth/tasks";
const SCOPE_TASKS_READONLY: &str = "https://www.googleapis.com/auth/tasks.readonly";

pub struct TasksHelper;

fn list_id_arg() -> Arg {
    Arg::new("list-id")
        .long("list-id")
        .help("Task list ID (@default is your default list)")
        .default_value("@default")
        .value_name("ID")
}

impl Helper for TasksHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+add")
                .about("[Helper] Add a task")
                .arg(list_id_arg())
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Task title")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("notes")
                        .long("notes")
                        .help("Task notes")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("due")
                        .long("due")
                        .help("Due date, YYYY-MM-DD (Google Tasks does not store a time of day)")
                        .value_name("DATE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr tasks +add --title 'Send invoice'
  gwsr tasks +add --title 'File taxes' --due 2026-04-15 --notes 'Use the new form'

TIPS:
  The Tasks API keeps only the due date; a --due with a time other than
  midnight is rejected rather than silently truncated.",
                ),
        )
        .subcommand(
            Command::new("+list")
                .about("[Helper] List tasks in a task list")
                .arg(list_id_arg())
                .arg(
                    Arg::new("show-completed")
                        .long("show-completed")
                        .help("Include completed tasks")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum tasks (default: all)")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr tasks +list
  gwsr tasks +list --show-completed --format table

TIPS:
  Read-only. Fetches every page unless --limit is given.",
                ),
        )
        .subcommand(
            Command::new("+lists")
                .about("[Helper] List your task lists")
                .after_help("EXAMPLES:\n  gwsr tasks +lists --format table"),
        )
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            let Some((name, m)) = matches.subcommand() else {
                return Ok(false);
            };
            let dry = crate::args::dry_run(m)?;
            let (api, value) = match name {
                "+add" => {
                    let due = optional(m, "due")?.map(parse_due).transpose()?;
                    let api = Api::new(doc, &[SCOPE_TASKS], dry, sanitize).await?;
                    let v = add(
                        &api,
                        required(m, "list-id")?,
                        required(m, "title")?,
                        optional(m, "notes")?,
                        due.as_deref(),
                    )
                    .await?;
                    (api, v)
                }
                "+list" => {
                    let limit = http::limit(m, "limit")?;
                    let api = Api::new(doc, &[SCOPE_TASKS_READONLY], dry, sanitize).await?;
                    let v = list(
                        &api,
                        required(m, "list-id")?,
                        flag(m, "show-completed")?,
                        limit,
                    )
                    .await?;
                    (api, v)
                }
                "+lists" => {
                    let api = Api::new(doc, &[SCOPE_TASKS_READONLY], dry, sanitize).await?;
                    let page = api
                        .paginate(
                            ApiRequest::get(api.url("tasks/v1/users/@me/lists"))
                                .query("maxResults", "100"),
                            "items",
                            None,
                        )
                        .await?;
                    (api, page.into_json("lists"))
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

/// Convert `--due` into the RFC 3339 midnight-UTC form the API expects.
fn parse_due(s: &str) -> Result<String, GwsError> {
    use chrono::{NaiveDate, NaiveDateTime, Timelike};
    // Each accepted form is tried in turn; input matching none is reported.
    let date = if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        d
    } else {
        let (date, has_time) = if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
            (
                t.date_naive(),
                t.time().num_seconds_from_midnight() != 0 || t.offset().local_minus_utc() != 0,
            )
        } else if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
            .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        {
            (t.date(), t.time().num_seconds_from_midnight() != 0)
        } else {
            return Err(GwsError::Validation(format!(
                "--due '{s}' is not a date; use YYYY-MM-DD"
            )));
        };
        if has_time {
            return Err(GwsError::Validation(format!(
                "--due '{s}' has a time of day, but Google Tasks only stores the due date and would silently drop the time; pass just the date ({date}) or put the time in --notes"
            )));
        }
        date
    };
    Ok(format!("{}T00:00:00.000Z", date.format("%Y-%m-%d")))
}

async fn add(
    api: &Api,
    list_id: &str,
    title: &str,
    notes: Option<&str>,
    due: Option<&str>,
) -> Result<Value, GwsError> {
    let mut body = json!({ "title": title });
    if let Some(n) = notes {
        body["notes"] = json!(n);
    }
    if let Some(d) = due {
        body["due"] = json!(d);
    }
    api.send(
        ApiRequest::post(api.url(&format!(
            "tasks/v1/lists/{}/tasks",
            encode_path_segment(list_id)
        )))
        .json(body),
    )
    .await
}

async fn list(
    api: &Api,
    list_id: &str,
    show_completed: bool,
    limit: Option<usize>,
) -> Result<Value, GwsError> {
    let req = ApiRequest::get(api.url(&format!(
        "tasks/v1/lists/{}/tasks",
        encode_path_segment(list_id)
    )))
    .query("maxResults", "100")
    .query("showCompleted", show_completed.to_string())
    .query("showHidden", show_completed.to_string());
    let page = api.paginate(req, "items", limit).await?;
    Ok(page.into_json("tasks"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn due_accepts_dates_and_rejects_times() {
        assert_eq!(parse_due("2026-04-15").unwrap(), "2026-04-15T00:00:00.000Z");
        assert_eq!(
            parse_due("2026-04-15T00:00:00Z").unwrap(),
            "2026-04-15T00:00:00.000Z"
        );
        assert_eq!(
            parse_due("2026-04-15T00:00").unwrap(),
            "2026-04-15T00:00:00.000Z"
        );
        let err = parse_due("2026-04-15T17:00:00Z").unwrap_err().to_string();
        assert!(err.contains("only stores the due date"), "{err}");
        assert!(parse_due("2026-04-15T00:00:00-07:00").is_err());
        assert!(parse_due("2026-04-15T09:30").is_err());
        assert!(parse_due("next friday").is_err());
    }

    #[tokio::test]
    async fn add_sends_due_date() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/tasks/v1/lists/@default/tasks"))
            .and(body_json(
                json!({"title": "T", "due": "2026-04-15T00:00:00.000Z"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        add(
            &api,
            "@default",
            "T",
            None,
            Some("2026-04-15T00:00:00.000Z"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_paginates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tasks/v1/lists/L1/tasks"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [{"id": "b"}]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tasks/v1/lists/L1/tasks"))
            .and(query_param("showCompleted", "false"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [{"id": "a"}], "nextPageToken": "p2"})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert_eq!(list(&api, "L1", false, None).await.unwrap()["count"], 2);
    }

    #[tokio::test]
    async fn show_completed_also_shows_hidden() {
        // Upstream googleworkspace/cli#509: tasks completed in Google's apps are
        // hidden, so showCompleted alone returned nothing.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tasks/v1/lists/L1/tasks"))
            .and(query_param("showCompleted", "true"))
            .and(query_param("showHidden", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"items": [{"id": "a", "status": "completed", "hidden": true}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert_eq!(list(&api, "L1", true, None).await.unwrap()["count"], 1);
    }

    #[tokio::test]
    async fn add_plan_in_dry_run() {
        let api = dry_api("");
        add(&api, "@default", "T", Some("n"), None).await.unwrap();
        assert_eq!(
            api.planned()[0]["body"],
            json!({"title": "T", "notes": "n"})
        );
    }
}
