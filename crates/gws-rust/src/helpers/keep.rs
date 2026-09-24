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

//! Keep helpers: `+list`.

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, optional};
use crate::error::GwsError;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

const SCOPE_KEEP_READONLY: &str = "https://www.googleapis.com/auth/keep.readonly";

pub struct KeepHelper;

impl Helper for KeepHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+list")
                .about("[Helper] List notes")
                .arg(
                    Arg::new("trashed")
                        .long("trashed")
                        .help("List trashed notes instead")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("changed-since")
                        .long("changed-since")
                        .help("Only notes changed after this RFC 3339 time")
                        .value_name("TIME"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum notes (default: all)")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr keep +list
  gwsr keep +list --changed-since 2026-01-01T00:00:00Z

TIPS:
  Read-only. The Keep API is only available to Workspace accounts and
  usually requires a service account with domain-wide delegation.",
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
            let Some(("+list", m)) = matches.subcommand() else {
                return Ok(false);
            };
            let limit = http::limit(m, "limit")?;
            let filter = build_filter(flag(m, "trashed")?, optional(m, "changed-since")?)?;
            let api = Api::new(
                doc,
                &[SCOPE_KEEP_READONLY],
                crate::args::dry_run(m)?,
                sanitize,
            )
            .await?;
            let v = list(&api, &filter, limit).await?;
            api.emit(m, &v).await?;
            Ok(true)
        })
    }
}

fn build_filter(trashed: bool, changed_since: Option<&str>) -> Result<String, GwsError> {
    let mut parts = vec![format!("trashed = {trashed}")];
    if let Some(t) = changed_since {
        let ts = chrono::DateTime::parse_from_rfc3339(t).map_err(|_| {
            GwsError::Validation(format!("--changed-since '{t}' is not an RFC 3339 time"))
        })?;
        parts.push(format!(
            "update_time > \"{}\"",
            ts.with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ));
    }
    Ok(parts.join(" AND "))
}

async fn list(api: &Api, filter: &str, limit: Option<usize>) -> Result<Value, GwsError> {
    let req = ApiRequest::get(api.url("v1/notes"))
        .query("filter", filter)
        .query("pageSize", "100");
    let page = api.paginate(req, "notes", limit).await?;
    Ok(page.into_json("notes"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::http::test_support::dry_api;
    use super::*;

    #[test]
    fn filter_building() {
        assert_eq!(build_filter(false, None).unwrap(), "trashed = false");
        assert_eq!(
            build_filter(true, Some("2026-01-01T01:00:00+01:00")).unwrap(),
            "trashed = true AND update_time > \"2026-01-01T00:00:00Z\""
        );
        assert!(build_filter(false, Some("yesterday")).is_err());
    }

    #[tokio::test]
    async fn list_plans_request() {
        let api = dry_api("");
        list(&api, "trashed = false", Some(5)).await.unwrap();
        assert_eq!(
            api.planned()[0]["query_params"]["filter"],
            "trashed = false"
        );
    }
}
