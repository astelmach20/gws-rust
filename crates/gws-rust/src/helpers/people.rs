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

//! People helpers: `+find`.

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, required};
use crate::error::GwsError;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const SCOPE_CONTACTS_READONLY: &str = "https://www.googleapis.com/auth/contacts.readonly";
const SCOPE_DIRECTORY_READONLY: &str = "https://www.googleapis.com/auth/directory.readonly";
const READ_MASK: &str = "names,emailAddresses,phoneNumbers,organizations";

pub struct PeopleHelper;

impl Helper for PeopleHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+find")
                .about("[Helper] Find people in your contacts or your organization's directory")
                .arg(
                    Arg::new("query")
                        .long("query")
                        .help("Name, email or phone prefix to search for")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("directory")
                        .long("directory")
                        .help("Search the Workspace directory instead of your contacts")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum results (contacts: at most 30)")
                        .default_value("30")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr people +find --query ann
  gwsr people +find --query 'ann@example.com' --directory --format table

TIPS:
  Read-only. Contact search matches name/email/phone prefixes.",
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
            let Some(("+find", m)) = matches.subcommand() else {
                return Ok(false);
            };
            let directory = flag(m, "directory")?;
            let limit = http::limit(m, "limit")?;
            let scope = if directory {
                SCOPE_DIRECTORY_READONLY
            } else {
                SCOPE_CONTACTS_READONLY
            };
            let api = Api::new(doc, &[scope], crate::args::dry_run(m)?, sanitize).await?;
            let v = find(&api, required(m, "query")?, directory, limit).await?;
            api.emit(m, &v).await?;
            Ok(true)
        })
    }
}

fn values(person: &Value, key: &str, field: &str) -> Vec<Value> {
    person
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get(field).cloned()).collect())
        .unwrap_or_default()
}

fn summarize(person: &Value) -> Value {
    json!({
        "resourceName": person.get("resourceName"),
        "name": person.pointer("/names/0/displayName"),
        "emails": values(person, "emailAddresses", "value"),
        "phones": values(person, "phoneNumbers", "value"),
        "organization": person.pointer("/organizations/0/name"),
        "title": person.pointer("/organizations/0/title"),
    })
}

async fn find(
    api: &Api,
    query: &str,
    directory: bool,
    limit: Option<usize>,
) -> Result<Value, GwsError> {
    if query.trim().is_empty() {
        return Err(GwsError::Validation("--query must not be empty".into()));
    }
    if directory {
        let req = ApiRequest::get(api.url("v1/people:searchDirectoryPeople"))
            .query("query", query)
            .query("readMask", READ_MASK)
            .query("sources", "DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE")
            .query("sources", "DIRECTORY_SOURCE_TYPE_DOMAIN_CONTACT")
            .query("pageSize", limit.unwrap_or(30).min(500).to_string());
        let mut page = api.paginate(req, "people", limit).await?;
        page.items = page.items.iter().map(summarize).collect();
        return Ok(page.into_json("people"));
    }
    let size = limit.unwrap_or(30);
    if size > 30 {
        return Err(GwsError::Validation(
            "Contact search returns at most 30 results; use --limit 30 or less".into(),
        ));
    }
    // The API requires a warm-up request with an empty query to refresh its
    // cache before the real search returns current results.
    api.send(
        ApiRequest::get(api.url("v1/people:searchContacts"))
            .query("query", "")
            .query("readMask", READ_MASK),
    )
    .await?;
    let resp = api
        .send(
            ApiRequest::get(api.url("v1/people:searchContacts"))
                .query("query", query)
                .query("readMask", READ_MASK)
                .query("pageSize", size.to_string()),
        )
        .await?;
    let people: Vec<Value> = resp
        .get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get("person"))
                .map(summarize)
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "people": people, "count": people.len() }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::api;
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn contacts_search_warms_up_then_queries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/people:searchContacts"))
            .and(query_param("query", ""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/people:searchContacts"))
            .and(query_param("query", "ann"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"results": [{"person": {
                    "resourceName": "people/1",
                    "names": [{"displayName": "Ann Lee"}],
                    "emailAddresses": [{"value": "ann@x.com"}]
                }}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let v = find(&api, "ann", false, Some(10)).await.unwrap();
        assert_eq!(v["people"][0]["name"], "Ann Lee");
        assert_eq!(v["people"][0]["emails"], json!(["ann@x.com"]));
    }

    #[tokio::test]
    async fn directory_search_uses_sources() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/people:searchDirectoryPeople"))
            .and(query_param(
                "sources",
                "DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"people": [{"names": [{"displayName": "B"}]}]})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert_eq!(find(&api, "b", true, None).await.unwrap()["count"], 1);
    }

    #[tokio::test]
    async fn contact_limit_and_empty_query_validation() {
        let api = super::super::http::test_support::dry_api("");
        assert!(find(&api, "a", false, Some(31)).await.is_err());
        assert!(find(&api, " ", false, None).await.is_err());
    }
}
