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

//! Classroom helpers: `+courses`.

use super::Helper;
use super::http::{self, Api, ApiRequest, optional};
use crate::error::GwsError;
use clap::{Arg, ArgMatches, Command};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

const SCOPE_COURSES_READONLY: &str = "https://www.googleapis.com/auth/classroom.courses.readonly";

pub struct ClassroomHelper;

impl Helper for ClassroomHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+courses")
                .about("[Helper] List your courses")
                .arg(
                    Arg::new("role")
                        .long("role")
                        .help("Only courses where you are a teacher or a student")
                        .value_parser(["teacher", "student"])
                        .value_name("ROLE"),
                )
                .arg(
                    Arg::new("state")
                        .long("state")
                        .help("Only courses in this state (default: all)")
                        .value_parser([
                            "active",
                            "archived",
                            "provisioned",
                            "declined",
                            "suspended",
                        ])
                        .value_name("STATE"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum courses (default: all)")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gwsr classroom +courses
  gwsr classroom +courses --role teacher --state active --format table

TIPS:
  Read-only. Fetches every page unless --limit is given.",
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
            let Some(("+courses", m)) = matches.subcommand() else {
                return Ok(false);
            };
            let limit = http::limit(m, "limit")?;
            let api = Api::new(doc, &[SCOPE_COURSES_READONLY], http::dry_run(m), sanitize).await?;
            let v = courses(&api, optional(m, "role"), optional(m, "state"), limit).await?;
            api.emit(m, &v).await?;
            Ok(true)
        })
    }
}

async fn courses(
    api: &Api,
    role: Option<&str>,
    state: Option<&str>,
    limit: Option<usize>,
) -> Result<Value, GwsError> {
    let mut req = ApiRequest::get(api.url("v1/courses")).query("pageSize", "100");
    match role {
        Some("teacher") => req = req.query("teacherId", "me"),
        Some(_) => req = req.query("studentId", "me"),
        None => {}
    }
    if let Some(s) = state {
        req = req.query("courseStates", s.to_ascii_uppercase());
    }
    let page = api.paginate(req, "courses", limit).await?;
    Ok(page.into_json("courses"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::http::test_support::dry_api;
    use super::*;

    #[tokio::test]
    async fn filters_map_to_query() {
        let api = dry_api("");
        courses(&api, Some("teacher"), Some("active"), None)
            .await
            .unwrap();
        let q = &api.planned()[0]["query"];
        assert_eq!(q["teacherId"], "me");
        assert_eq!(q["courseStates"], "ACTIVE");
    }
}
