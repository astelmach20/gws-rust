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

//! Chat helpers: `+send`, `+spaces`, `+read`.

use super::Helper;
use super::confirm::{self, Impact, with_yes};
use super::http::{self, Api, ApiRequest, optional, required};
use crate::error::GwsError;
use clap::{Arg, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_MESSAGES_CREATE: &str = "https://www.googleapis.com/auth/chat.messages.create";
const SCOPE_MESSAGES_READONLY: &str = "https://www.googleapis.com/auth/chat.messages.readonly";
const SCOPE_SPACES_READONLY: &str = "https://www.googleapis.com/auth/chat.spaces.readonly";

pub struct ChatHelper;

fn space_id_arg() -> Arg {
    Arg::new("space-id")
        .long("space-id")
        .help("Space ID, as 'AAAA...' or 'spaces/AAAA...'")
        .required(true)
        .value_name("ID")
}

/// Normalize a space ID to the `spaces/{id}` resource name, rejecting
/// anything that is not a plain ID (path traversal, query injection, ...).
fn space_name(raw: &str) -> Result<String, GwsError> {
    let id = raw.strip_prefix("spaces/").unwrap_or(raw);
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(GwsError::Validation(format!(
            "Invalid --space-id '{raw}'; expected e.g. spaces/AAAAxxxx"
        )));
    }
    Ok(format!("spaces/{id}"))
}

/// Validate a thread resource name belonging to `space`.
fn thread_name(raw: &str, space: &str) -> Result<String, GwsError> {
    let rest = raw
        .strip_prefix(&format!("{space}/threads/"))
        .ok_or_else(|| {
            GwsError::Validation(format!(
                "--thread must look like {space}/threads/THREAD_ID, got '{raw}'"
            ))
        })?;
    if rest.is_empty()
        || !rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(GwsError::Validation(format!("Invalid --thread '{raw}'")));
    }
    Ok(raw.to_string())
}

impl Helper for ChatHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(with_yes(
            Command::new("+send")
                .about("[Helper] Send a message to a space")
                .arg(space_id_arg())
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Message text (Chat formatting such as *bold* is supported)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("thread")
                        .long("thread")
                        .help("Reply in this thread (spaces/SPACE/threads/THREAD); falls back to a new thread if it no longer exists")
                        .value_name("NAME"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr chat +send --space-id spaces/AAAAxxxx --text 'Hello team!'
  gwsr chat +send --space-id AAAAxxxx --thread spaces/AAAAxxxx/threads/TTTT --text 'Done.'

TIPS:
  Use 'gwsr chat +spaces' to find space IDs.
  Requires confirmation when GWSR_REQUIRE_CONFIRM=1.",
                ),
        ))
        .subcommand(
            Command::new("+spaces")
                .about("[Helper] List the spaces you are a member of")
                .arg(
                    Arg::new("type")
                        .long("type")
                        .help("Only this kind of space")
                        .value_parser(["space", "group-chat", "direct-message"])
                        .value_name("TYPE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr chat +spaces
  gwsr chat +spaces --type space --format table

TIPS:
  Read-only. All pages are fetched.",
                ),
        )
        .subcommand(
            Command::new("+read")
                .about("[Helper] Read the most recent messages in a space")
                .arg(space_id_arg())
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Number of messages (default: 25)")
                        .default_value("25")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr chat +read --space-id spaces/AAAAxxxx
  gwsr chat +read --space-id AAAAxxxx --limit 100 --format table

TIPS:
  Read-only. Newest messages first; the output says when more exist.",
                ),
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
            let dry = http::dry_run(m);
            let (api, value) = match name {
                "+send" => {
                    let space = space_name(required(m, "space-id")?)?;
                    let thread = optional(m, "thread")
                        .map(|t| thread_name(t, &space))
                        .transpose()?;
                    confirm::gate(m, Impact::Outbound, &format!("post a message to {space}"))?;
                    let api = Api::new(doc, &[SCOPE_MESSAGES_CREATE], dry, sanitize).await?;
                    let v = send(&api, &space, required(m, "text")?, thread.as_deref()).await?;
                    (api, v)
                }
                "+spaces" => {
                    let api = Api::new(doc, &[SCOPE_SPACES_READONLY], dry, sanitize).await?;
                    let v = spaces(&api, optional(m, "type")).await?;
                    (api, v)
                }
                "+read" => {
                    let space = space_name(required(m, "space-id")?)?;
                    let limit = http::limit(m, "limit")?;
                    let api = Api::new(doc, &[SCOPE_MESSAGES_READONLY], dry, sanitize).await?;
                    let v = read(&api, &space, limit).await?;
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

async fn send(api: &Api, space: &str, text: &str, thread: Option<&str>) -> Result<Value, GwsError> {
    if text.trim().is_empty() {
        return Err(GwsError::Validation("--text must not be empty".into()));
    }
    let mut body = json!({ "text": text });
    let mut req = ApiRequest::post(api.url(&format!("v1/{space}/messages")));
    if let Some(t) = thread {
        body["thread"] = json!({ "name": t });
        req = req.query("messageReplyOption", "REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD");
    }
    api.send(req.json(body)).await
}

async fn spaces(api: &Api, kind: Option<&str>) -> Result<Value, GwsError> {
    let filter = kind.map(|k| {
        let t = match k {
            "space" => "SPACE",
            "group-chat" => "GROUP_CHAT",
            _ => "DIRECT_MESSAGE",
        };
        format!("spaceType = \"{t}\"")
    });
    let page = api
        .paginate(
            ApiRequest::get(api.url("v1/spaces"))
                .query("pageSize", "1000")
                .query_opt("filter", filter),
            "spaces",
            None,
        )
        .await?;
    Ok(page.into_json("spaces"))
}

async fn read(api: &Api, space: &str, limit: Option<usize>) -> Result<Value, GwsError> {
    let page_size = limit.unwrap_or(25).min(1000).to_string();
    let page = api
        .paginate(
            ApiRequest::get(api.url(&format!("v1/{space}/messages")))
                .query("orderBy", "createTime desc")
                .query("pageSize", page_size),
            "messages",
            limit,
        )
        .await?;
    Ok(page.into_json("messages"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn space_name_normalizes_and_validates() {
        assert_eq!(space_name("AAA-1_b").unwrap(), "spaces/AAA-1_b");
        assert_eq!(space_name("spaces/AAA").unwrap(), "spaces/AAA");
        for bad in [
            "",
            "spaces/",
            "../etc/passwd",
            "spaces/AAA?key=x",
            "spaces/A/B",
            "A%2F",
        ] {
            assert!(space_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn thread_must_belong_to_space() {
        assert!(thread_name("spaces/A/threads/T1", "spaces/A").is_ok());
        assert!(thread_name("spaces/B/threads/T1", "spaces/A").is_err());
        assert!(thread_name("spaces/A/threads/../x", "spaces/A").is_err());
    }

    #[tokio::test]
    async fn send_posts_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/spaces/AAA/messages"))
            .and(body_json(json!({"text": "hi"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"name": "spaces/AAA/messages/1"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        send(&api, "spaces/AAA", "hi", None).await.unwrap();
        assert!(send(&api, "spaces/AAA", "  ", None).await.is_err());
    }

    #[tokio::test]
    async fn send_thread_reply_sets_option() {
        let api = dry_api("");
        send(&api, "spaces/A", "x", Some("spaces/A/threads/T"))
            .await
            .unwrap();
        let plan = api.planned();
        assert_eq!(
            plan[0]["query"]["messageReplyOption"],
            "REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD"
        );
        assert_eq!(plan[0]["body"]["thread"]["name"], "spaces/A/threads/T");
    }

    #[tokio::test]
    async fn read_limits_explicitly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/spaces/A/messages"))
            .and(query_param("orderBy", "createTime desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{"text": "a"}, {"text": "b"}], "nextPageToken": "n"
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = read(&api, "spaces/A", Some(2)).await.unwrap();
        assert_eq!(v["count"], 2);
        assert_eq!(v["truncated"], true);
    }

    #[tokio::test]
    async fn spaces_filter() {
        let api = dry_api("");
        spaces(&api, Some("group-chat")).await.unwrap();
        assert_eq!(
            api.planned()[0]["query"]["filter"],
            "spaceType = \"GROUP_CHAT\""
        );
    }
}
