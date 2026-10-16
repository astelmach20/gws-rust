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

//! Admin SDK helpers.
//!
//! Directory API (`admin` `directory_v1`): `+user-create`, `+user-suspend`,
//! `+group-add-member`. Reports API (`admin` `reports_v1`): `+audit`.
//! Both APIs share the Discovery name `admin`, so the commands injected depend
//! on the document version.

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, optional, required};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use zeroize::Zeroizing;

const SCOPE_USER: &str = "https://www.googleapis.com/auth/admin.directory.user";
const SCOPE_GROUP_MEMBER: &str = "https://www.googleapis.com/auth/admin.directory.group.member";
const SCOPE_AUDIT: &str = "https://www.googleapis.com/auth/admin.reports.audit.readonly";

const DIRECTORY: &str = "directory_v1";
const REPORTS: &str = "reports_v1";

/// The applications `activities.list` accepts, as the Discovery document
/// lists them. Hard-coding them would refuse every application Google adds
/// after a release (`contacts`, `takeout`, `voice`, ...).
fn audit_apps(doc: &crate::discovery::RestDescription) -> Option<Vec<String>> {
    doc.resources
        .get("activities")?
        .methods
        .get("list")?
        .parameters
        .get("applicationName")?
        .enum_values
        .clone()
}

pub struct AdminHelper;

impl Helper for AdminHelper {
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command {
        match doc.version.as_str() {
            DIRECTORY => directory_commands(cmd),
            REPORTS => reports_commands(cmd, doc),
            _ => cmd,
        }
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
            // Extra context when the output is withheld after the write.
            let mut withheld_note = None;
            let (api, value) = match name {
                "+user-create" => {
                    let args = UserCreate::parse(m)?;
                    let api = Api::new(doc, &[SCOPE_USER], dry, sanitize).await?;
                    let v = user_create(&api, &args).await?;
                    if args.generated {
                        withheld_note = Some(
                            "The user was created but its generated initial password cannot be \
                             shown; set a new password for the user",
                        );
                    }
                    (api, v)
                }
                "+user-suspend" => {
                    let user = required(m, "user")?;
                    let suspend = !flag(m, "unsuspend")?;
                    let (impact, action) = if suspend {
                        (
                            Impact::Destructive,
                            format!("suspend {user} (signs them out and blocks access)"),
                        )
                    } else {
                        (Impact::Outbound, format!("restore access for {user}"))
                    };
                    confirm::confirm(m, impact, &action)?;
                    let api = Api::new(doc, &[SCOPE_USER], dry, sanitize).await?;
                    let v = user_suspend(&api, user, suspend).await?;
                    (api, v)
                }
                "+group-add-member" => {
                    let group = required(m, "group")?;
                    let member = required(m, "member")?;
                    let role = required(m, "role")?;
                    confirm::confirm(
                        m,
                        Impact::Outbound,
                        &format!("add {member} to {group} as {role}"),
                    )?;
                    let api = Api::new(doc, &[SCOPE_GROUP_MEMBER], dry, sanitize).await?;
                    let v = group_add_member(&api, group, member, role).await?;
                    (api, v)
                }
                "+audit" => {
                    let args = Audit::parse(m)?;
                    let api = Api::new(doc, &[SCOPE_AUDIT], dry, sanitize).await?;
                    let v = audit(&api, &args).await?;
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await.map_err(|e| match withheld_note {
                Some(note) => crate::helpers::modelarmor::prefix_message(e, note),
                None => e,
            })?;
            Ok(true)
        })
    }
}

fn directory_commands(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("+user-create")
            .about("[Helper] Create a user account")
            .arg(Arg::new("email").long("email").help("Primary email").required(true).value_name("EMAIL"))
            .arg(Arg::new("given-name").long("given-name").help("First name").required(true).value_name("NAME"))
            .arg(Arg::new("family-name").long("family-name").help("Last name").required(true).value_name("NAME"))
            .arg(
                Arg::new("org-unit")
                    .long("org-unit")
                    .help("Organizational unit path (default: /)")
                    .value_name("PATH"),
            )
            .arg(
                Arg::new("password-file")
                    .long("password-file")
                    .help("Read the initial password from a file, or '-' for stdin (default: generate one)")
                    .value_name("PATH"),
            )
            .arg(
                Arg::new("no-password-change")
                    .long("no-password-change")
                    .help("Do not force a password change at first sign-in")
                    .action(ArgAction::SetTrue),
            )
            .after_help(
                "\
EXAMPLES:
  gwsr admin +user-create --email ann@example.com --given-name Ann --family-name Lee
  printf '%s' \"$PW\" | gwsr admin +user-create --email ann@example.com --given-name Ann --family-name Lee --password-file -

TIPS:
  Without --password-file a random 24-character password is generated and
  printed once as initialPassword; share it over a secure channel.
  The user must change the password at first sign-in unless --no-password-change.",
            ),
    )
    .subcommand(with_yes(
        Command::new("+user-suspend")
            .about("[Helper] Suspend (or with --unsuspend, restore) a user")
            .arg(Arg::new("user").long("user").help("User email or ID").required(true).value_name("EMAIL"))
            .arg(
                Arg::new("unsuspend")
                    .long("unsuspend")
                    .help("Restore a suspended user instead")
                    .action(ArgAction::SetTrue),
            )
            .after_help(
                "\
EXAMPLES:
  gwsr admin +user-suspend --user ann@example.com --yes
  gwsr admin +user-suspend --user ann@example.com --unsuspend

TIPS:
  Suspending requires --yes (or a prompt on a terminal).
  The Directory API cannot record a suspension reason (its suspensionReason
  field is output-only).",
            ),
    ))
    .subcommand(with_yes(
        Command::new("+group-add-member")
            .about("[Helper] Add a user or group to a group")
            .arg(Arg::new("group").long("group").help("Group email or ID").required(true).value_name("EMAIL"))
            .arg(Arg::new("member").long("member").help("Member email").required(true).value_name("EMAIL"))
            .arg(
                Arg::new("role")
                    .long("role")
                    .help("Membership role")
                    .value_parser(["MEMBER", "MANAGER", "OWNER"])
                    .default_value("MEMBER")
                    .value_name("ROLE"),
            )
            .after_help(
                "\
EXAMPLES:
  gwsr admin +group-add-member --group eng@example.com --member ann@example.com
  gwsr admin +group-add-member --group eng@example.com --member lead@example.com --role MANAGER

TIPS:
  Adding someone who is already a member fails with the API's 409 error.",
            ),
    ))
}

fn reports_commands(cmd: Command, doc: &crate::discovery::RestDescription) -> Command {
    let mut application = Arg::new("application")
        .long("application")
        .help("Application whose audit log to read")
        .required(true)
        .value_name("APP");
    // Without an enum in the document the API validates the name; the path
    // segment is encoded either way.
    if let Some(apps) = audit_apps(doc) {
        application = application.value_parser(clap::builder::PossibleValuesParser::new(apps));
    }
    cmd.subcommand(
        Command::new("+audit")
            .about("[Helper] Query audit activity (Reports API)")
            .arg(application)
            .arg(
                Arg::new("user")
                    .long("user")
                    .help("Only this user's activity (email or ID; default: all users)")
                    .value_name("EMAIL"),
            )
            .arg(
                Arg::new("event")
                    .long("event")
                    .help("Only this event name, e.g. login_failure")
                    .value_name("NAME"),
            )
            .arg(
                Arg::new("since")
                    .long("since")
                    .help("Start: RFC 3339 time, or a look-back like 24h / 7d (default: 1d)")
                    .default_value("1d")
                    .value_name("WHEN"),
            )
            .arg(
                Arg::new("until")
                    .long("until")
                    .help("End: RFC 3339 time (default: now)")
                    .value_name("TIME"),
            )
            .arg(
                Arg::new("filter")
                    .long("filter")
                    .help("Event parameter filter, e.g. 'doc_id==XYZ'")
                    .value_name("EXPR"),
            )
            .arg(
                Arg::new("limit")
                    .long("limit")
                    .help("Maximum activities (default: 1000)")
                    .default_value("1000")
                    .value_name("N"),
            )
            .after_help(
                "\
EXAMPLES:
  gwsr admin-reports +audit --application login --event login_failure --since 7d
  gwsr admin-reports +audit --application drive --user ann@example.com --since 2026-06-01T00:00:00Z
  gwsr admin-reports +audit --application admin --format table

TIPS:
  Read-only. Newest first. The output says when --limit cut results short.
  Gmail queries cover at most 30 days; --until defaults to now.",
            ),
    )
}

// ── +user-create ─────────────────────────────────────────────────────

struct UserCreate {
    email: String,
    given: String,
    family: String,
    org_unit: Option<String>,
    password: Zeroizing<String>,
    generated: bool,
    change_at_next_login: bool,
}

fn generate_password() -> Zeroizing<String> {
    use rand::RngExt;
    Zeroizing::new(
        rand::rng()
            .sample_iter(&rand::distr::Alphanumeric)
            .take(24)
            .map(char::from)
            .collect(),
    )
}

impl UserCreate {
    fn parse(m: &ArgMatches) -> Result<Self, GwsError> {
        let (password, generated) = match optional(m, "password-file")? {
            Some(path) => {
                let raw = Zeroizing::new(http::read_text_input(path, "--password-file")?);
                let pw = Zeroizing::new(raw.trim_end_matches(['\r', '\n']).to_string());
                if pw.chars().count() < 8 {
                    return Err(GwsError::Validation(
                        "The password must be at least 8 characters".into(),
                    ));
                }
                (pw, false)
            }
            None => (generate_password(), true),
        };
        Ok(Self {
            email: required(m, "email")?.to_string(),
            given: required(m, "given-name")?.to_string(),
            family: required(m, "family-name")?.to_string(),
            org_unit: optional(m, "org-unit")?.map(str::to_string),
            password,
            generated,
            change_at_next_login: !flag(m, "no-password-change")?,
        })
    }
}

async fn user_create(api: &Api, args: &UserCreate) -> Result<Value, GwsError> {
    let mut body = json!({
        "primaryEmail": args.email,
        "name": { "givenName": args.given, "familyName": args.family },
        "password": args.password.as_str(),
        "changePasswordAtNextLogin": args.change_at_next_login,
    });
    if let Some(ou) = &args.org_unit {
        body["orgUnitPath"] = json!(ou);
    }
    let mut req = ApiRequest::post(api.url("admin/directory/v1/users")).json(body);
    if api.is_dry_run()
        && let http::Body::Json(b) = &mut req.body
    {
        b["password"] = json!("<redacted>");
    }
    let created = api.send(req).await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    let mut out = json!({
        "id": created.get("id"),
        "primaryEmail": created.get("primaryEmail"),
        "orgUnitPath": created.get("orgUnitPath"),
        "changePasswordAtNextLogin": args.change_at_next_login,
    });
    if args.generated {
        out["initialPassword"] = json!(args.password.as_str());
    }
    Ok(out)
}

// ── +user-suspend / +group-add-member ────────────────────────────────

async fn user_suspend(api: &Api, user: &str, suspend: bool) -> Result<Value, GwsError> {
    // Only `suspended` is writable: the API's `suspensionReason` is output-only.
    let body = json!({ "suspended": suspend });
    let resp = api
        .send(
            ApiRequest::put(api.url(&format!(
                "admin/directory/v1/users/{}",
                encode_path_segment(user)
            )))
            .json(body),
        )
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    Ok(json!({ "primaryEmail": resp.get("primaryEmail"), "suspended": resp.get("suspended") }))
}

async fn group_add_member(
    api: &Api,
    group: &str,
    member: &str,
    role: &str,
) -> Result<Value, GwsError> {
    api.send(
        ApiRequest::post(api.url(&format!(
            "admin/directory/v1/groups/{}/members",
            encode_path_segment(group)
        )))
        .json(json!({ "email": member, "role": role })),
    )
    .await
}

// ── +audit ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct Audit {
    application: String,
    user: String,
    event: Option<String>,
    start: String,
    end: Option<String>,
    filter: Option<String>,
    limit: Option<usize>,
}

/// An RFC 3339 time in UTC, keeping its fractional seconds: truncating them
/// would move `--since` earlier (returning events before it) and `--until`
/// earlier (dropping events in the range's last second).
fn utc_rfc3339(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// `--until`: an RFC 3339 time.
fn until_time(s: &str) -> Result<chrono::DateTime<chrono::Utc>, GwsError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&chrono::Utc))
        .map_err(|_| GwsError::Validation(format!("--until '{s}' is not an RFC 3339 time")))
}

#[cfg(test)]
fn parse_until(s: &str) -> Result<String, GwsError> {
    until_time(s).map(utc_rfc3339)
}

#[cfg(test)]
fn parse_since(s: &str, now: chrono::DateTime<chrono::Utc>) -> Result<String, GwsError> {
    since_time(s, now).map(utc_rfc3339)
}

/// `--since`: RFC 3339, or a look-back like `24h`/`7d`/`30m` relative to `now`
/// (whole seconds).
fn since_time(
    s: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, GwsError> {
    use chrono::SubsecRound;
    // Not RFC 3339 is not an error yet: it may be a look-back, parsed next.
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&chrono::Utc));
    }
    let invalid = || {
        GwsError::Validation(format!(
            "--since '{s}' is not RFC 3339 or a look-back like 24h/7d"
        ))
    };
    // Split off the last character, not the last byte: the input may be
    // arbitrary Unicode.
    let mut chars = s.chars();
    let unit = chars.next_back().ok_or_else(invalid)?;
    let n: i64 = chars.as_str().parse().map_err(|_| invalid())?;
    let d = match unit {
        'm' => chrono::TimeDelta::try_minutes(n),
        'h' => chrono::TimeDelta::try_hours(n),
        'd' => chrono::TimeDelta::try_days(n),
        _ => return Err(invalid()),
    };
    if n <= 0 {
        return Err(GwsError::Validation(
            "--since look-back must be positive".into(),
        ));
    }
    let start = d.and_then(|d| now.checked_sub_signed(d)).ok_or_else(|| {
        GwsError::Validation(format!("--since '{s}' reaches too far into the past"))
    })?;
    Ok(start.trunc_subsecs(0))
}

/// The Reports API refuses Gmail queries without an `endTime`, and Gmail
/// ranges longer than this.
const GMAIL_MAX_RANGE_DAYS: i64 = 30;

impl Audit {
    fn parse(m: &ArgMatches) -> Result<Self, GwsError> {
        Self::parse_at(m, chrono::Utc::now())
    }

    fn parse_at(m: &ArgMatches, now: chrono::DateTime<chrono::Utc>) -> Result<Self, GwsError> {
        let application = required(m, "application")?;
        let start = since_time(required(m, "since")?, now)?;
        let mut end = optional(m, "until")?.map(until_time).transpose()?;
        if application == "gmail" {
            // Gmail requires both ends of the range: default the end to now,
            // as the API does for every other application.
            let until = *end.get_or_insert(now);
            if until - start > chrono::TimeDelta::days(GMAIL_MAX_RANGE_DAYS) {
                return Err(GwsError::Validation(format!(
                    "Gmail audit queries cover at most {GMAIL_MAX_RANGE_DAYS} days; \
                     narrow --since/--until"
                )));
            }
        }
        Ok(Self {
            application: application.to_string(),
            user: optional(m, "user")?.unwrap_or("all").to_string(),
            event: optional(m, "event")?.map(str::to_string),
            start: utc_rfc3339(start),
            end: end.map(utc_rfc3339),
            filter: optional(m, "filter")?.map(str::to_string),
            limit: http::limit(m, "limit")?,
        })
    }
}

async fn audit(api: &Api, a: &Audit) -> Result<Value, GwsError> {
    let page_size = a.limit.unwrap_or(1000).min(1000).to_string();
    let req = ApiRequest::get(api.url(&format!(
        "admin/reports/v1/activity/users/{}/applications/{}",
        encode_path_segment(&a.user),
        encode_path_segment(&a.application)
    )))
    .query("startTime", a.start.clone())
    .query_opt("endTime", a.end.clone())
    .query_opt("eventName", a.event.clone())
    .query_opt("filters", a.filter.clone())
    .query("maxResults", page_size);
    let page = api.paginate(req, "items", a.limit).await?;
    Ok(page.into_json("activities"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn doc(version: &str) -> crate::discovery::RestDescription {
        crate::discovery::RestDescription {
            name: "admin".into(),
            version: version.into(),
            ..Default::default()
        }
    }

    fn names(version: &str) -> Vec<String> {
        AdminHelper
            .inject_commands(Command::new("gwsr"), &doc(version))
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect()
    }

    #[test]
    fn commands_depend_on_api_version() {
        assert_eq!(
            names(DIRECTORY),
            vec!["+user-create", "+user-suspend", "+group-add-member"]
        );
        assert_eq!(names(REPORTS), vec!["+audit"]);
        assert!(names("datatransfer_v1").is_empty());
    }

    /// A Reports document whose `activities.list` accepts `apps`.
    fn reports_doc(apps: &[&str]) -> crate::discovery::RestDescription {
        serde_json::from_value(json!({
            "name": "admin",
            "version": REPORTS,
            "rootUrl": "https://admin.googleapis.com/",
            "servicePath": "",
            "resources": {"activities": {"methods": {"list": {
                "id": "reports.activities.list",
                "httpMethod": "GET",
                "path": "admin/reports/v1/activity/users/{userKey}/applications/{applicationName}",
                "parameters": {"applicationName": {
                    "type": "string", "location": "path", "required": true, "enum": apps
                }}
            }}}}
        }))
        .unwrap()
    }

    fn audit_app(
        doc: &crate::discovery::RestDescription,
        app: &str,
    ) -> Result<String, clap::Error> {
        AdminHelper
            .inject_commands(Command::new("gwsr"), doc)
            .try_get_matches_from(["gwsr", "+audit", "--application", app])
            .map(|m| {
                m.subcommand_matches("+audit")
                    .unwrap()
                    .get_one::<String>("application")
                    .unwrap()
                    .clone()
            })
    }

    #[test]
    fn audit_applications_come_from_discovery() {
        // Applications the API added after the helper was written.
        let reports = reports_doc(&["login", "contacts", "takeout", "voice"]);
        for app in ["login", "contacts", "takeout", "voice"] {
            assert_eq!(audit_app(&reports, app).unwrap(), app);
        }
        let err = audit_app(&reports, "drive").unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        // No enum published: the API validates the name.
        assert_eq!(audit_app(&doc(REPORTS), "contacts").unwrap(), "contacts");
    }

    #[test]
    fn generated_passwords_are_strong_and_unique() {
        let a = generate_password();
        let b = generate_password();
        assert_eq!(a.len(), 24);
        assert_ne!(a.as_str(), b.as_str());
    }

    #[tokio::test]
    async fn user_create_returns_generated_password_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/admin/directory/v1/users"))
            .and(body_partial_json(json!({
                "primaryEmail": "ann@x.com", "changePasswordAtNextLogin": true,
                "name": {"givenName": "Ann", "familyName": "Lee"}
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": "1", "primaryEmail": "ann@x.com"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let args = UserCreate {
            email: "ann@x.com".into(),
            given: "Ann".into(),
            family: "Lee".into(),
            org_unit: None,
            password: Zeroizing::new("S3cretpassword!".into()),
            generated: true,
            change_at_next_login: true,
        };
        let v = user_create(&api, &args).await.unwrap();
        assert_eq!(v["initialPassword"], "S3cretpassword!");
    }

    #[tokio::test]
    async fn user_create_dry_run_redacts_password() {
        let api = dry_api("");
        let args = UserCreate {
            email: "a@x".into(),
            given: "A".into(),
            family: "B".into(),
            org_unit: Some("/Eng".into()),
            password: Zeroizing::new("supersecret".into()),
            generated: false,
            change_at_next_login: false,
        };
        user_create(&api, &args).await.unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["body"]["password"], "<redacted>");
        assert_eq!(plan[0]["body"]["orgUnitPath"], "/Eng");
    }

    #[tokio::test]
    async fn suspend_and_group_requests() {
        let api = dry_api("");
        user_suspend(&api, "ann@x.com", true).await.unwrap();
        group_add_member(&api, "eng@x.com", "ann@x.com", "MANAGER")
            .await
            .unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["method"], "PUT");
        assert!(
            plan[0]["url"]
                .as_str()
                .unwrap()
                .ends_with("/admin/directory/v1/users/ann@x.com")
        );
        assert_eq!(plan[0]["body"], json!({"suspended": true}));
        assert_eq!(
            plan[1]["body"],
            json!({"email": "ann@x.com", "role": "MANAGER"})
        );
    }

    /// The `User` properties `+user-suspend` could write, as published in the
    /// Directory API Discovery document (`admin:directory_v1`).
    fn directory_user_schema() -> Value {
        json!({
            "suspended": {"type": "boolean", "description": "Indicates if user is suspended."},
            "suspensionReason": {
                "readOnly": true,
                "type": "string",
                "description": "Output only. Has the reason a user account is suspended either by the administrator or by Google at the time of suspension."
            }
        })
    }

    #[tokio::test]
    async fn suspend_writes_only_writable_user_fields() {
        let schema = directory_user_schema();
        for suspend in [true, false] {
            let api = dry_api("");
            user_suspend(&api, "ann@x.com", suspend).await.unwrap();
            let body = api.planned()[0]["body"].clone();
            for key in body.as_object().unwrap().keys() {
                let prop = &schema[key];
                assert!(prop.is_object(), "unknown User field {key}");
                assert_ne!(
                    prop["readOnly"], true,
                    "{key} is output-only: the API ignores it"
                );
            }
        }
    }

    #[test]
    fn suspend_has_no_reason_flag() {
        let cmd = AdminHelper.inject_commands(Command::new("gwsr"), &doc(DIRECTORY));
        let err = cmd
            .try_get_matches_from([
                "gwsr",
                "+user-suspend",
                "--user",
                "ann@x.com",
                "--reason",
                "offboarding",
            ])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn since_parsing() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-10T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(parse_since("7d", now).unwrap(), "2026-01-03T00:00:00Z");
        assert_eq!(
            parse_since("2026-01-01T05:00:00+01:00", now).unwrap(),
            "2026-01-01T04:00:00Z"
        );
        for bad in ["", "7", "7w", "-1d", "0h"] {
            assert!(parse_since(bad, now).is_err(), "{bad}");
        }
    }

    #[test]
    fn since_and_until_keep_fractional_seconds() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-10T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        // Truncating --until to whole seconds would drop events in the last
        // sub-second of the range; truncating --since would add earlier ones.
        assert_eq!(
            parse_until("2026-01-01T12:34:56.789Z").unwrap(),
            "2026-01-01T12:34:56.789Z"
        );
        assert_eq!(
            parse_until("2026-01-01T13:34:56.5+01:00").unwrap(),
            "2026-01-01T12:34:56.500Z"
        );
        assert_eq!(
            parse_since("2026-01-01T12:34:56.123456Z", now).unwrap(),
            "2026-01-01T12:34:56.123456Z"
        );
        assert_eq!(
            parse_until("2026-01-01T12:34:56Z").unwrap(),
            "2026-01-01T12:34:56Z"
        );
        assert!(matches!(
            parse_until("tomorrow"),
            Err(GwsError::Validation(_))
        ));
    }

    #[test]
    fn since_rejects_non_ascii_and_out_of_range_without_panicking() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-10T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        // A multi-byte last character, a look-back chrono cannot represent,
        // and one that lands before the earliest representable date must be
        // validation errors (exit 3), not panics (exit 101).
        for bad in [
            "7é",
            "1日",
            "99999999999999d",
            "9223372036854775807m",
            "999999999d",
            "100000000d",
        ] {
            let err = parse_since(bad, now).unwrap_err();
            assert!(matches!(err, GwsError::Validation(_)), "{bad}: {err:?}");
        }
    }

    fn parse_audit(args: &[&str], now: &str) -> Result<Audit, GwsError> {
        let now = chrono::DateTime::parse_from_rfc3339(now)
            .unwrap()
            .with_timezone(&chrono::Utc);
        let m = AdminHelper
            .inject_commands(Command::new("gwsr"), &doc(REPORTS))
            .try_get_matches_from(["gwsr", "+audit"].iter().chain(args))
            .unwrap();
        Audit::parse_at(m.subcommand_matches("+audit").unwrap(), now)
    }

    #[test]
    fn gmail_audit_always_sends_an_end_time() {
        // The Reports API rejects Gmail queries without endTime.
        let a = parse_audit(&["--application", "gmail"], "2026-01-10T08:00:00Z").unwrap();
        assert_eq!(a.start, "2026-01-09T08:00:00Z");
        assert_eq!(a.end.as_deref(), Some("2026-01-10T08:00:00Z"));
        let a = parse_audit(
            &[
                "--application",
                "gmail",
                "--until",
                "2026-01-10T01:00:00+01:00",
            ],
            "2026-01-10T08:00:00Z",
        )
        .unwrap();
        assert_eq!(a.end.as_deref(), Some("2026-01-10T00:00:00Z"));
        // Other applications keep the API's own default (now).
        let a = parse_audit(&["--application", "drive"], "2026-01-10T08:00:00Z").unwrap();
        assert_eq!(a.end, None);
    }

    #[test]
    fn gmail_audit_range_is_at_most_30_days() {
        let now = "2026-02-01T00:00:00Z";
        assert!(parse_audit(&["--application", "gmail", "--since", "30d"], now).is_ok());
        let err = parse_audit(&["--application", "gmail", "--since", "31d"], now).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        let err = parse_audit(
            &[
                "--application",
                "gmail",
                "--since",
                "2025-12-01T00:00:00Z",
                "--until",
                "2026-01-01T00:00:01Z",
            ],
            now,
        )
        .unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)), "{err:?}");
        // The limit is Gmail's alone.
        assert!(parse_audit(&["--application", "drive", "--since", "90d"], now).is_ok());
    }

    #[tokio::test]
    async fn audit_paginates_with_filters() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/admin/reports/v1/activity/users/all/applications/login",
            ))
            .and(query_param("eventName", "login_failure"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [{"id": {}}]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let a = Audit {
            application: "login".into(),
            user: "all".into(),
            event: Some("login_failure".into()),
            start: "2026-01-01T00:00:00Z".into(),
            end: None,
            filter: None,
            limit: None,
        };
        assert_eq!(audit(&api, &a).await.unwrap()["count"], 1);
    }
}
