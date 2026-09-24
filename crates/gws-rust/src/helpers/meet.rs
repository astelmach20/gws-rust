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

//! Meet helpers: `+create`.

use super::Helper;
use super::http::{Api, ApiRequest};
use crate::args::optional;
use crate::error::GwsError;
use clap::{Arg, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_SPACE_CREATED: &str = "https://www.googleapis.com/auth/meetings.space.created";

pub struct MeetHelper;

impl Helper for MeetHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+create")
                .about("[Helper] Create a Meet meeting space and print its link")
                .arg(
                    Arg::new("access-type")
                        .long("access-type")
                        .help(
                            "Who can join without knocking (default: your organization's setting)",
                        )
                        .value_parser(["open", "trusted", "restricted"])
                        .value_name("TYPE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr meet +create
  gwsr meet +create --access-type restricted

TIPS:
  Prints meetingUri and meetingCode. To attach a Meet link to a calendar
  event use `gwsr calendar +insert --meet` instead.",
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
            let Some(("+create", m)) = matches.subcommand() else {
                return Ok(false);
            };
            let api = Api::new(
                doc,
                &[SCOPE_SPACE_CREATED],
                crate::args::dry_run(m)?,
                sanitize,
            )
            .await?;
            let v = create(&api, optional(m, "access-type")?).await?;
            api.emit(m, &v).await?;
            Ok(true)
        })
    }
}

async fn create(api: &Api, access: Option<&str>) -> Result<Value, GwsError> {
    let mut body = json!({});
    if let Some(a) = access {
        body["config"] = json!({ "accessType": a.to_ascii_uppercase() });
    }
    let resp = api
        .send(ApiRequest::post(api.url("v2/spaces")).json(body))
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    Ok(json!({
        "name": resp.get("name"),
        "meetingUri": resp.get("meetingUri"),
        "meetingCode": resp.get("meetingCode"),
        "accessType": resp.pointer("/config/accessType"),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn create_returns_link() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/spaces"))
            .and(body_json(json!({"config": {"accessType": "RESTRICTED"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "spaces/abc", "meetingUri": "https://meet.google.com/abc-defg-hij", "meetingCode": "abc-defg-hij"
            })))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = create(&api, Some("restricted")).await.unwrap();
        assert_eq!(v["meetingCode"], "abc-defg-hij");
    }

    #[tokio::test]
    async fn create_default_body_is_empty() {
        let api = dry_api("");
        create(&api, None).await.unwrap();
        assert_eq!(api.planned()[0]["body"], json!({}));
    }
}
