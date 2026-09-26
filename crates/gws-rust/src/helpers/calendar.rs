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

//! Calendar helpers: `+insert`, `+agenda`, `+freebusy`, `+update`, `+delete`,
//! `+rsvp`.

mod time;

use super::Helper;
use super::http::{self, Api, ApiRequest};
use crate::args::{flag, many, optional, required};
use crate::confirm::{self, Impact, with_yes};
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use chrono_tz::Tz;
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use time::When;

const SCOPE_CALENDAR: &str = "https://www.googleapis.com/auth/calendar";
const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
const SCOPE_FREEBUSY: &str = "https://www.googleapis.com/auth/calendar.freebusy";

pub struct CalendarHelper;

fn calendar_id_arg() -> Arg {
    Arg::new("calendar-id")
        .long("calendar-id")
        .help("Calendar ID")
        .default_value("primary")
        .value_name("ID")
}

fn event_id_arg() -> Arg {
    Arg::new("event-id")
        .long("event-id")
        .help("Event ID")
        .required(true)
        .value_name("ID")
}

fn timezone_arg() -> Arg {
    Arg::new("timezone")
        .long("timezone")
        .help("IANA time zone for times without a UTC offset (default: your Google account time zone)")
        .value_name("TZ")
}

fn send_updates_arg() -> Arg {
    Arg::new("send-updates")
        .long("send-updates")
        .help("Who gets email notifications")
        .value_parser(["all", "externalOnly", "none"])
        .default_value("all")
        .value_name("WHO")
}

fn start_end_args(cmd: Command, required: bool) -> Command {
    cmd.arg(
        Arg::new("start")
            .long("start")
            .help("Start: 2026-06-17 (all-day), 2026-06-17T09:00 (in --timezone) or RFC 3339 with offset")
            .required(required)
            .value_name("TIME"),
    )
    .arg(
        Arg::new("end")
            .long("end")
            .help("End, same forms as --start (all-day end dates are exclusive)")
            .value_name("TIME"),
    )
    .arg(
        Arg::new("duration")
            .long("duration")
            .help("Length instead of --end, e.g. 30m, 1h, 1h30m")
            .conflicts_with("end")
            .value_name("DURATION"),
    )
}

impl Helper for CalendarHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(with_yes(start_end_args(
            Command::new("+insert")
                .about("[Helper] Create a new event")
                .arg(calendar_id_arg())
                .arg(
                    Arg::new("summary")
                        .long("summary")
                        .help("Event title")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(Arg::new("location").long("location").help("Event location").value_name("TEXT"))
                .arg(
                    Arg::new("description")
                        .long("description")
                        .help("Event description")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("attendee")
                        .long("attendee")
                        .help("Attendee email (repeatable)")
                        .value_name("EMAIL")
                        .action(ArgAction::Append),
                )
                .arg(
                    Arg::new("meet")
                        .long("meet")
                        .help("Add a Google Meet link")
                        .action(ArgAction::SetTrue),
                )
                .arg(timezone_arg())
                .arg(send_updates_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +insert --summary 'Standup' --start '2026-06-17T09:00' --duration 30m
  gwsr calendar +insert --summary 'Review' --start '2026-06-17T09:00:00-07:00' --end '2026-06-17T10:00:00-07:00' --attendee alice@example.com --meet
  gwsr calendar +insert --summary 'Offsite' --start 2026-06-17 --end 2026-06-19

TIPS:
  Times without an offset use --timezone, else your account time zone.
  A date-only --start creates an all-day event (--end defaults to the next day).
  Timed events need --end or --duration.
  Invitations are emailed to attendees unless --send-updates none.",
                ),
            true,
        )))
        .subcommand(
            Command::new("+agenda")
                .about("[Helper] Show upcoming events across calendars")
                .arg(Arg::new("today").long("today").help("Today's events").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("tomorrow")
                        .long("tomorrow")
                        .help("Tomorrow's events")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("week")
                        .long("week")
                        .help("The next 7 days")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("days")
                        .long("days")
                        .help("The next N days (default: 1)")
                        .value_parser(clap::value_parser!(u32).range(1..=366))
                        .value_name("N"),
                )
                .group(ArgGroup::new("window").args(["today", "tomorrow", "week", "days"]))
                .arg(
                    Arg::new("calendar-id")
                        .long("calendar-id")
                        .help("Only this calendar (repeatable)")
                        .action(ArgAction::Append)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("calendar-name")
                        .long("calendar-name")
                        .help("Only calendars whose name contains TEXT")
                        .value_name("TEXT"),
                )
                .arg(timezone_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +agenda
  gwsr calendar +agenda --today
  gwsr calendar +agenda --week --format table
  gwsr calendar +agenda --days 3 --calendar-name Work
  gwsr calendar +agenda --today --timezone America/New_York

TIPS:
  Read-only. Every page of every selected calendar is fetched (no truncation);
  a calendar that cannot be read fails the command with its name.",
                ),
        )
        .subcommand(start_end_args(
            Command::new("+freebusy")
                .about("[Helper] Show busy times and find free slots")
                .arg(
                    Arg::new("calendar-id")
                        .long("calendar-id")
                        .help("Calendar or person email to check (repeatable; default: primary)")
                        .action(ArgAction::Append)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("slot")
                        .long("slot")
                        .help("Also list common free slots at least this long, e.g. 30m")
                        .value_name("DURATION"),
                )
                .arg(
                    Arg::new("working-hours")
                        .long("working-hours")
                        .help("Restrict free slots to these daily hours, e.g. 09:00-17:00")
                        .requires("slot")
                        .value_name("HH:MM-HH:MM"),
                )
                .arg(timezone_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +freebusy --start 2026-06-17 --end 2026-06-18
  gwsr calendar +freebusy --start 2026-06-17T09:00 --duration 8h --calendar-id alice@example.com --calendar-id bob@example.com --slot 30m
  gwsr calendar +freebusy --start 2026-06-15 --end 2026-06-20 --calendar-id primary --calendar-id alice@example.com --slot 1h --working-hours 09:00-17:00

TIPS:
  Read-only. A calendar you cannot see fails the command (no silent gaps).
  Slot times are reported in --timezone (default: account time zone).",
                ),
            true,
        ))
        .subcommand(with_yes(start_end_args(
            Command::new("+update")
                .about("[Helper] Change fields of an existing event")
                .arg(calendar_id_arg())
                .arg(event_id_arg())
                .arg(Arg::new("summary").long("summary").help("New title").value_name("TEXT"))
                .arg(Arg::new("location").long("location").help("New location").value_name("TEXT"))
                .arg(
                    Arg::new("description")
                        .long("description")
                        .help("New description")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("add-attendee")
                        .long("add-attendee")
                        .help("Invite this email (repeatable)")
                        .action(ArgAction::Append)
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("remove-attendee")
                        .long("remove-attendee")
                        .help("Uninvite this email (repeatable)")
                        .action(ArgAction::Append)
                        .value_name("EMAIL"),
                )
                .arg(timezone_arg())
                .arg(send_updates_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +update --event-id EVENT_ID --summary 'New title'
  gwsr calendar +update --event-id EVENT_ID --start 2026-06-17T10:00 --duration 45m
  gwsr calendar +update --event-id EVENT_ID --add-attendee carol@example.com --remove-attendee bob@example.com

TIPS:
  Only the given fields change. Moving --start without --end/--duration keeps
  the event's current length.",
                ),
            false,
        )))
        .subcommand(with_yes(
            Command::new("+delete")
                .about("[Helper] Delete an event")
                .arg(calendar_id_arg())
                .arg(event_id_arg())
                .arg(send_updates_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +delete --event-id EVENT_ID --yes
  gwsr calendar +delete --event-id EVENT_ID --send-updates none --yes

TIPS:
  Destructive: requires --yes (or a confirmation prompt on a terminal).
  Attendees are notified of the cancellation unless --send-updates none.",
                ),
        ))
        .subcommand(with_yes(
            Command::new("+rsvp")
                .about("[Helper] Respond to an event invitation")
                .arg(calendar_id_arg())
                .arg(event_id_arg())
                .arg(
                    Arg::new("response")
                        .long("response")
                        .help("Your response")
                        .required(true)
                        .value_parser(["accepted", "declined", "tentative"])
                        .value_name("RESPONSE"),
                )
                .arg(
                    Arg::new("comment")
                        .long("comment")
                        .help("Note to the organizer")
                        .value_name("TEXT"),
                )
                .arg(send_updates_arg())
                .after_help(
                    "\
EXAMPLES:
  gwsr calendar +rsvp --event-id EVENT_ID --response accepted
  gwsr calendar +rsvp --event-id EVENT_ID --response declined --comment 'Out that day'

TIPS:
  Fails if you are not on the event's guest list.",
                ),
        ))
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
                "+insert" => {
                    let attendees = many(m, "attendee")?;
                    let send_updates = required(m, "send-updates")?;
                    if !attendees.is_empty() && send_updates != "none" {
                        confirm::confirm(
                            m,
                            Impact::Outbound,
                            &format!("email invitations to {}", attendees.join(", ")),
                        )?;
                    }
                    let api = Api::new(doc, &[SCOPE_CALENDAR], dry, sanitize).await?;
                    let tz = TzInfo::resolve(&api, optional(m, "timezone")?).await?;
                    let v = insert(&api, &InsertArgs::parse(m, &tz)?).await?;
                    (api, v)
                }
                "+agenda" => {
                    let api = Api::new(doc, &[SCOPE_CALENDAR_READONLY], dry, sanitize).await?;
                    let tz = TzInfo::resolve(&api, optional(m, "timezone")?).await?;
                    let v = agenda(&api, &AgendaArgs::parse(m)?, &tz).await?;
                    (api, v)
                }
                "+freebusy" => {
                    let api = Api::new(doc, &[SCOPE_FREEBUSY], dry, sanitize).await?;
                    let tz = TzInfo::resolve(&api, optional(m, "timezone")?).await?;
                    let v = freebusy(&api, m, &tz).await?;
                    (api, v)
                }
                "+update" => {
                    confirm::confirm(
                        m,
                        Impact::Outbound,
                        &format!("update event {}", required(m, "event-id")?),
                    )?;
                    let api = Api::new(doc, &[SCOPE_CALENDAR], dry, sanitize).await?;
                    let tz = TzInfo::resolve(&api, optional(m, "timezone")?).await?;
                    let v = update(&api, m, &tz).await?;
                    (api, v)
                }
                "+delete" => {
                    let event = required(m, "event-id")?;
                    confirm::confirm(m, Impact::Destructive, &format!("delete event {event}"))?;
                    let api = Api::new(doc, &[SCOPE_CALENDAR], dry, sanitize).await?;
                    let v = delete(
                        &api,
                        required(m, "calendar-id")?,
                        event,
                        required(m, "send-updates")?,
                    )
                    .await?;
                    (api, v)
                }
                "+rsvp" => {
                    let response = required(m, "response")?;
                    let event = required(m, "event-id")?;
                    confirm::confirm(
                        m,
                        Impact::Outbound,
                        &format!("send RSVP '{response}' for event {event}"),
                    )?;
                    let api = Api::new(doc, &[SCOPE_CALENDAR], dry, sanitize).await?;
                    let v = rsvp(
                        &api,
                        required(m, "calendar-id")?,
                        event,
                        response,
                        optional(m, "comment")?,
                        required(m, "send-updates")?,
                    )
                    .await?;
                    (api, v)
                }
                _ => return Ok(false),
            };
            api.emit(m, &value).await?;
            Ok(true)
        })
    }
}

/// The time zone used to interpret local times.
struct TzInfo {
    tz: Tz,
    /// IANA name sent to the API.
    name: String,
    /// Whether the user passed --timezone.
    explicit: bool,
}

impl TzInfo {
    async fn resolve(api: &Api, flag_value: Option<&str>) -> Result<Self, GwsError> {
        if let Some(name) = flag_value {
            return Ok(Self {
                tz: crate::timezone::parse_timezone(name)?,
                name: name.to_string(),
                explicit: true,
            });
        }
        match api.transport() {
            Some(transport) => {
                let tz = crate::timezone::resolve_account_timezone(transport, None).await?;
                Ok(Self {
                    tz,
                    name: tz.name().to_string(),
                    explicit: false,
                })
            }
            None => {
                api.plan_note(json!({
                    "note": "times without an offset use the account time zone at run time; this preview uses UTC (pass --timezone to choose)"
                }))?;
                Ok(Self {
                    tz: chrono_tz::UTC,
                    name: "UTC".into(),
                    explicit: false,
                })
            }
        }
    }

    fn for_event(&self, start: &When) -> Option<&str> {
        (self.explicit || start.is_local()).then_some(self.name.as_str())
    }
}

fn events_url(api: &Api, calendar_id: &str) -> String {
    api.url(&format!(
        "calendars/{}/events",
        encode_path_segment(calendar_id)
    ))
}

fn event_url(api: &Api, calendar_id: &str, event_id: &str) -> String {
    api.url(&format!(
        "calendars/{}/events/{}",
        encode_path_segment(calendar_id),
        encode_path_segment(event_id)
    ))
}

/// Resolve `--start` plus `--end`/`--duration` into a validated pair.
fn resolve_range(
    start: &str,
    end: Option<&str>,
    duration: Option<&str>,
    tz: Tz,
) -> Result<(When, When), GwsError> {
    let start = time::parse_when(start, "--start")?;
    let end = match (end, duration) {
        (Some(e), _) => time::parse_when(e, "--end")?,
        (None, Some(d)) => start.plus(time::parse_duration(d)?)?,
        (None, None) => match &start {
            When::Date(d) => When::Date(*d + chrono::Duration::days(1)),
            _ => {
                return Err(GwsError::Validation(
                    "Timed events need --end or --duration".into(),
                ));
            }
        },
    };
    time::check_range(&start, &end, tz)?;
    Ok((start, end))
}

// ── +insert ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct InsertArgs {
    calendar_id: String,
    body: Value,
    send_updates: String,
    meet: bool,
}

impl InsertArgs {
    fn parse(m: &ArgMatches, tz: &TzInfo) -> Result<Self, GwsError> {
        let (start, end) = resolve_range(
            required(m, "start")?,
            optional(m, "end")?,
            optional(m, "duration")?,
            tz.tz,
        )?;
        let summary = required(m, "summary")?;
        let mut attendees = many(m, "attendee")?;
        let mut body = json!({
            "summary": summary,
            "start": start.to_event_time(tz.for_event(&start)),
            "end": end.to_event_time(tz.for_event(&start)),
        });
        if let Some(loc) = optional(m, "location")? {
            body["location"] = json!(loc);
        }
        if let Some(desc) = optional(m, "description")? {
            body["description"] = json!(desc);
        }
        if !attendees.is_empty() {
            body["attendees"] = attendees.iter().map(|e| json!({ "email": e })).collect();
        }
        let meet = flag(m, "meet")?;
        if meet {
            // Deterministic request ID so a retried insert reuses the conference.
            attendees.sort();
            let seed = json!({
                "v": 2,
                "calendar": required(m, "calendar-id")?,
                "summary": summary,
                "start": body["start"],
                "end": body["end"],
                "attendees": attendees,
            });
            let seed = serde_json::to_vec(&seed).map_err(http::other_err)?;
            let request_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, &seed).to_string();
            body["conferenceData"] = json!({
                "createRequest": {
                    "requestId": request_id,
                    "conferenceSolutionKey": { "type": "hangoutsMeet" }
                }
            });
        }
        Ok(Self {
            calendar_id: required(m, "calendar-id")?.to_string(),
            body,
            send_updates: required(m, "send-updates")?.to_string(),
            meet,
        })
    }
}

async fn insert(api: &Api, args: &InsertArgs) -> Result<Value, GwsError> {
    let mut req = ApiRequest::post(events_url(api, &args.calendar_id))
        .query("sendUpdates", args.send_updates.clone())
        .json(args.body.clone());
    if args.meet {
        req = req.query("conferenceDataVersion", "1");
    }
    api.send(req).await
}

// ── +agenda ──────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Window {
    Today,
    Tomorrow,
    Days(u32),
}

#[derive(Debug)]
struct AgendaArgs {
    window: Window,
    calendar_ids: Vec<String>,
    calendar_name: Option<String>,
}

impl AgendaArgs {
    fn parse(m: &ArgMatches) -> Result<Self, GwsError> {
        let window = if flag(m, "today")? {
            Window::Today
        } else if flag(m, "tomorrow")? {
            Window::Tomorrow
        } else if flag(m, "week")? {
            Window::Days(7)
        } else {
            Window::Days(
                m.try_get_one::<u32>("days")
                    .map_err(http::other_err)?
                    .copied()
                    .unwrap_or(1),
            )
        };
        Ok(Self {
            window,
            calendar_ids: many(m, "calendar-id")?,
            calendar_name: optional(m, "calendar-name")?.map(str::to_string),
        })
    }
}

fn agenda_bounds(
    window: &Window,
    tz: Tz,
) -> Result<(chrono::DateTime<Tz>, chrono::DateTime<Tz>), GwsError> {
    let today = crate::timezone::start_of_today(tz)?;
    let day = chrono::Duration::days(1);
    Ok(match window {
        Window::Today => (today, today + day),
        Window::Tomorrow => (today + day, today + day * 2),
        Window::Days(n) => {
            let now = chrono::Utc::now().with_timezone(&tz);
            (now, now + day * i32::try_from(*n).map_err(http::other_err)?)
        }
    })
}

fn summarize_event(event: &Value, calendar_id: &str, calendar: &str) -> Value {
    let time = |k: &str| {
        event
            .get(k)
            .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "start": time("start"),
        "end": time("end"),
        "allDay": event.pointer("/start/date").is_some(),
        "summary": event.get("summary").and_then(Value::as_str).unwrap_or("(No title)"),
        "calendar": calendar,
        "calendarId": calendar_id,
        "location": event.get("location").and_then(Value::as_str).unwrap_or(""),
        "status": event.get("status"),
        "id": event.get("id"),
    })
}

/// Sort key: all-day dates sort at local midnight of their day. An event
/// whose start cannot be parsed still appears (its raw `start` is in the
/// output); it only sorts last, so ordering never hides an event.
fn start_key(event: &Value, tz: Tz) -> i64 {
    let s = event.get("start").and_then(Value::as_str).unwrap_or("");
    time::parse_when(s, "start")
        .and_then(|w| w.to_utc(tz))
        .map(|t| t.timestamp())
        .unwrap_or(i64::MAX)
}

async fn agenda(api: &Api, args: &AgendaArgs, tz: &TzInfo) -> Result<Value, GwsError> {
    let (min, max) = agenda_bounds(&args.window, tz.tz)?;
    let (time_min, time_max) = (min.to_rfc3339(), max.to_rfc3339());

    let calendars: Vec<(String, String)> = if api.is_dry_run() {
        api.paginate(
            ApiRequest::get(api.url("users/me/calendarList")),
            "items",
            None,
        )
        .await?;
        vec![("<each calendar>".into(), String::new())]
    } else {
        let list = api
            .paginate(
                ApiRequest::get(api.url("users/me/calendarList")).query("maxResults", "250"),
                "items",
                None,
            )
            .await?;
        let mut cals = Vec::new();
        // Which requested --calendar-id values matched a calendar-list entry.
        let mut id_found = vec![false; args.calendar_ids.len()];
        for cal in list.items {
            let id = cal
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| http::other_err(anyhow::anyhow!("calendarList entry without id")))?;
            let name = cal
                .get("summaryOverride")
                .or_else(|| cal.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or(id);
            let mut id_ok = args.calendar_ids.is_empty();
            for (c, found) in args.calendar_ids.iter().zip(id_found.iter_mut()) {
                if c == id || (c == "primary" && cal.get("primary") == Some(&json!(true))) {
                    *found = true;
                    id_ok = true;
                }
            }
            let name_ok = args
                .calendar_name
                .as_deref()
                .is_none_or(|n| name.to_lowercase().contains(&n.to_lowercase()));
            if id_ok && name_ok {
                cals.push((id.to_string(), name.to_string()));
            }
        }
        // A typo in one of several IDs must not silently shrink the agenda.
        let missing: Vec<&str> = args
            .calendar_ids
            .iter()
            .zip(&id_found)
            .filter(|(_, found)| !**found)
            .map(|(c, _)| c.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(GwsError::Validation(format!(
                "--calendar-id not found in your calendar list: {}",
                missing.join(", ")
            )));
        }
        if cals.is_empty() && (!args.calendar_ids.is_empty() || args.calendar_name.is_some()) {
            return Err(GwsError::Validation(
                "No calendar in your calendar list matches --calendar-id/--calendar-name".into(),
            ));
        }
        cals
    };

    use futures_util::stream::{self, StreamExt, TryStreamExt};
    let per_calendar: Vec<Vec<Value>> = stream::iter(calendars)
        .map(|(id, name)| {
            let (time_min, time_max) = (time_min.clone(), time_max.clone());
            let tz_name = tz.name.clone();
            async move {
                let req = ApiRequest::get(events_url(api, &id))
                    .query("timeMin", time_min)
                    .query("timeMax", time_max)
                    .query("timeZone", tz_name)
                    .query("singleEvents", "true")
                    .query("orderBy", "startTime")
                    .query("maxResults", "250");
                let page = api.paginate(req, "items", None).await.map_err(|e| {
                    http::other_err(anyhow::anyhow!(
                        "Failed to read calendar '{name}' ({id}): {e}"
                    ))
                })?;
                Ok::<_, GwsError>(
                    page.items
                        .iter()
                        .map(|e| summarize_event(e, &id, &name))
                        .collect(),
                )
            }
        })
        .buffer_unordered(5)
        .try_collect()
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    let mut events: Vec<Value> = per_calendar.into_iter().flatten().collect();
    events.sort_by_key(|e| start_key(e, tz.tz));
    Ok(json!({
        "events": events,
        "count": events.len(),
        "timeMin": time_min,
        "timeMax": time_max,
        "timeZone": tz.name,
    }))
}

// ── +freebusy ────────────────────────────────────────────────────────

async fn freebusy(api: &Api, m: &ArgMatches, tz: &TzInfo) -> Result<Value, GwsError> {
    let (start, end) = resolve_range(
        required(m, "start")?,
        optional(m, "end")?,
        optional(m, "duration")?,
        tz.tz,
    )?;
    let from = start.to_utc(tz.tz)?;
    let to = end.to_utc(tz.tz)?;
    let mut ids = many(m, "calendar-id")?;
    if ids.is_empty() {
        ids.push("primary".into());
    }
    let slot = optional(m, "slot")?.map(time::parse_duration).transpose()?;
    let working = optional(m, "working-hours")?
        .map(time::parse_working_hours)
        .transpose()?;
    let resp = api
        .send(ApiRequest::post(api.url("freeBusy")).json(json!({
            "timeMin": from.to_rfc3339(),
            "timeMax": to.to_rfc3339(),
            "timeZone": tz.name,
            "items": ids.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>(),
        })))
        .await?;
    if api.is_dry_run() {
        return Ok(Value::Null);
    }
    let cals = resp
        .get("calendars")
        .and_then(Value::as_object)
        .ok_or_else(|| http::other_err(anyhow::anyhow!("freeBusy response has no 'calendars'")))?;
    let mut all_busy = Vec::new();
    let mut failures = Vec::new();
    for (id, info) in cals {
        if let Some(errors) = info.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            let reasons: Vec<&str> = errors
                .iter()
                .filter_map(|e| e.get("reason").and_then(Value::as_str))
                .collect();
            failures.push(format!("{id} ({})", reasons.join(", ")));
            continue;
        }
        for b in info
            .get("busy")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let parse = |k: &str| {
                b.get(k)
                    .and_then(Value::as_str)
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .ok_or_else(|| {
                        http::other_err(anyhow::anyhow!("Malformed busy interval for {id}"))
                    })
            };
            all_busy.push((parse("start")?, parse("end")?));
        }
    }
    if !failures.is_empty() {
        return Err(GwsError::Validation(format!(
            "Free/busy unavailable for: {} (no access, or not a calendar)",
            failures.join("; ")
        )));
    }
    let mut out = json!({
        "timeMin": from.with_timezone(&tz.tz).to_rfc3339(),
        "timeMax": to.with_timezone(&tz.tz).to_rfc3339(),
        "timeZone": tz.name,
        "calendars": resp.get("calendars"),
    });
    if let Some(min) = slot {
        let slots = time::free_slots(from, to, all_busy, min, working, tz.tz);
        out["freeSlots"] = slots
            .into_iter()
            .map(|(s, e)| {
                json!({
                    "start": s.with_timezone(&tz.tz).to_rfc3339(),
                    "end": e.with_timezone(&tz.tz).to_rfc3339(),
                })
            })
            .collect();
    }
    Ok(out)
}

// ── +update ──────────────────────────────────────────────────────────

async fn update(api: &Api, m: &ArgMatches, tz: &TzInfo) -> Result<Value, GwsError> {
    let calendar_id = required(m, "calendar-id")?;
    let event_id = required(m, "event-id")?;
    let add = many(m, "add-attendee")?;
    let remove = many(m, "remove-attendee")?;
    let mut patch = serde_json::Map::new();
    for (flag_name, field) in [
        ("summary", "summary"),
        ("location", "location"),
        ("description", "description"),
    ] {
        if let Some(v) = optional(m, flag_name)? {
            patch.insert(field.into(), json!(v));
        }
    }
    let needs_current = !add.is_empty()
        || !remove.is_empty()
        || (optional(m, "start")?.is_some()
            && optional(m, "end")?.is_none()
            && optional(m, "duration")?.is_none())
        || (optional(m, "start")?.is_none()
            && (optional(m, "end")?.is_some() || optional(m, "duration")?.is_some()));
    let current = if needs_current && !api.is_dry_run() {
        Some(
            api.send(ApiRequest::get(event_url(api, calendar_id, event_id)))
                .await?,
        )
    } else {
        None
    };

    if optional(m, "start")?.is_some()
        || optional(m, "end")?.is_some()
        || optional(m, "duration")?.is_some()
    {
        let current_start = current
            .as_ref()
            .and_then(|c| {
                c.pointer("/start/dateTime")
                    .or_else(|| c.pointer("/start/date"))
            })
            .and_then(Value::as_str);
        let current_end = current
            .as_ref()
            .and_then(|c| {
                c.pointer("/end/dateTime")
                    .or_else(|| c.pointer("/end/date"))
            })
            .and_then(Value::as_str);
        let start_s = match (optional(m, "start")?, current_start) {
            (Some(s), _) => s.to_string(),
            (None, Some(s)) => s.to_string(),
            (None, None) => {
                return Err(GwsError::Validation(
                    "--end/--duration without --start needs the current event (not available in --dry-run)".into(),
                ));
            }
        };
        let (end_opt, dur_opt) = (optional(m, "end")?, optional(m, "duration")?);
        let (start, end) = if end_opt.is_none() && dur_opt.is_none() {
            // Keep the current length.
            let (cs, ce) = match (current_start, current_end) {
                (Some(cs), Some(ce)) => {
                    (time::parse_when(cs, "start")?, time::parse_when(ce, "end")?)
                }
                _ => {
                    return Err(GwsError::Validation(
                        "Changing --start alone keeps the event length, which needs the current event (pass --end or --duration with --dry-run)".into(),
                    ));
                }
            };
            let new_start = time::parse_when(&start_s, "--start")?;
            let end = match (&cs, &ce, &new_start) {
                (When::Date(a), When::Date(b), When::Date(n)) => When::Date(*n + (*b - *a)),
                (When::Date(_), _, _) | (_, _, When::Date(_)) => {
                    return Err(GwsError::Validation(
                        "Switching between all-day and timed needs an explicit --end".into(),
                    ));
                }
                _ => new_start.plus(ce.to_utc(tz.tz)? - cs.to_utc(tz.tz)?)?,
            };
            time::check_range(&new_start, &end, tz.tz)?;
            (new_start, end)
        } else {
            resolve_range(&start_s, end_opt, dur_opt, tz.tz)?
        };
        patch.insert("start".into(), start.to_event_time(tz.for_event(&start)));
        patch.insert("end".into(), end.to_event_time(tz.for_event(&start)));
    }

    if !add.is_empty() || !remove.is_empty() {
        let mut attendees: Vec<Value> = match &current {
            Some(c) => c
                .get("attendees")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            None => {
                api.plan_note(json!({"note": "attendee changes are merged into the event's current guest list at run time"}))?;
                Vec::new()
            }
        };
        let email_of = |a: &Value| {
            a.get("email")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
        };
        for r in &remove {
            let before = attendees.len();
            attendees.retain(|a| email_of(a) != r.to_lowercase());
            if before == attendees.len() && current.is_some() {
                return Err(GwsError::Validation(format!(
                    "{r} is not an attendee of this event"
                )));
            }
        }
        for a in &add {
            if !attendees.iter().any(|x| email_of(x) == a.to_lowercase()) {
                attendees.push(json!({ "email": a }));
            }
        }
        patch.insert("attendees".into(), Value::Array(attendees));
    }

    if patch.is_empty() {
        return Err(GwsError::Validation(
            "Nothing to update: pass at least one of --summary, --location, --description, --start, --end, --duration, --add-attendee, --remove-attendee".into(),
        ));
    }
    api.send(
        ApiRequest::patch(event_url(api, calendar_id, event_id))
            .query("sendUpdates", required(m, "send-updates")?)
            .json(Value::Object(patch)),
    )
    .await
}

// ── +delete / +rsvp ──────────────────────────────────────────────────

async fn delete(
    api: &Api,
    calendar_id: &str,
    event_id: &str,
    send_updates: &str,
) -> Result<Value, GwsError> {
    api.send(
        ApiRequest::delete(event_url(api, calendar_id, event_id))
            .query("sendUpdates", send_updates),
    )
    .await?;
    Ok(json!({ "deleted": true, "calendarId": calendar_id, "eventId": event_id }))
}

async fn rsvp(
    api: &Api,
    calendar_id: &str,
    event_id: &str,
    response: &str,
    comment: Option<&str>,
    send_updates: &str,
) -> Result<Value, GwsError> {
    let url = event_url(api, calendar_id, event_id);
    let attendees = if api.is_dry_run() {
        api.send(ApiRequest::get(url.clone())).await?;
        json!([{ "self": true, "responseStatus": response, "comment": comment }])
    } else {
        let event = api.send(ApiRequest::get(url.clone())).await?;
        let mut attendees = event
            .get("attendees")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let me = attendees
            .iter_mut()
            .find(|a| a.get("self").and_then(Value::as_bool) == Some(true))
            .ok_or_else(|| {
                GwsError::Validation(format!(
                    "You are not on the guest list of event {event_id}; nothing to respond to"
                ))
            })?;
        me["responseStatus"] = json!(response);
        if let Some(c) = comment {
            me["comment"] = json!(c);
        }
        Value::Array(attendees)
    };
    api.send(
        ApiRequest::patch(url)
            .query("sendUpdates", send_updates)
            .json(json!({ "attendees": attendees })),
    )
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::http::test_support::{api, dry_api};
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn parse(args: &[&str]) -> ArgMatches {
        CalendarHelper
            .inject_commands(
                Command::new("gwsr"),
                &crate::discovery::RestDescription::default(),
            )
            .try_get_matches_from(args)
            .unwrap()
    }

    fn tz(name: &str, explicit: bool) -> TzInfo {
        TzInfo {
            tz: name.parse().unwrap(),
            name: name.into(),
            explicit,
        }
    }

    fn insert_args(args: &[&str], tzi: &TzInfo) -> Result<InsertArgs, GwsError> {
        let m = parse(args);
        InsertArgs::parse(m.subcommand_matches("+insert").unwrap(), tzi)
    }

    #[test]
    fn insert_local_time_gets_time_zone() {
        // Regression (#912): naive times were sent without a timeZone -> HTTP 400.
        let a = insert_args(
            &[
                "gwsr",
                "+insert",
                "--summary",
                "S",
                "--start",
                "2026-03-18T14:00:00",
                "--duration",
                "30m",
            ],
            &tz("America/Denver", false),
        )
        .unwrap();
        assert_eq!(
            a.body["start"],
            json!({"dateTime": "2026-03-18T14:00:00", "timeZone": "America/Denver"})
        );
        assert_eq!(
            a.body["end"],
            json!({"dateTime": "2026-03-18T14:30:00", "timeZone": "America/Denver"})
        );
    }

    #[test]
    fn insert_all_day_defaults_end_to_next_day() {
        let a = insert_args(
            &["gwsr", "+insert", "--summary", "S", "--start", "2026-03-18"],
            &tz("UTC", false),
        )
        .unwrap();
        assert_eq!(a.body["start"], json!({"date": "2026-03-18"}));
        assert_eq!(a.body["end"], json!({"date": "2026-03-19"}));
    }

    #[test]
    fn insert_rejects_bad_ranges() {
        let t = tz("UTC", false);
        assert!(
            insert_args(
                &[
                    "gwsr",
                    "+insert",
                    "--summary",
                    "S",
                    "--start",
                    "2026-03-18T10:00:00Z"
                ],
                &t
            )
            .is_err()
        );
        assert!(
            insert_args(
                &[
                    "gwsr",
                    "+insert",
                    "--summary",
                    "S",
                    "--start",
                    "2026-03-18T10:00:00Z",
                    "--end",
                    "2026-03-18T09:00:00Z"
                ],
                &t
            )
            .is_err()
        );
    }

    #[test]
    fn insert_meet_request_id_is_deterministic_and_order_independent() {
        let t = tz("UTC", false);
        let base = [
            "gwsr",
            "+insert",
            "--summary",
            "S",
            "--start",
            "2026-01-01T10:00:00Z",
            "--duration",
            "1h",
            "--meet",
        ];
        let a = insert_args(
            &[&base[..], &["--attendee", "a@x", "--attendee", "b@x"]].concat(),
            &t,
        )
        .unwrap();
        let b = insert_args(
            &[&base[..], &["--attendee", "b@x", "--attendee", "a@x"]].concat(),
            &t,
        )
        .unwrap();
        let id = |x: &InsertArgs| x.body["conferenceData"]["createRequest"]["requestId"].clone();
        assert_eq!(id(&a), id(&b));
        assert!(a.meet);
        let c = insert_args(
            &[
                "gwsr",
                "+insert",
                "--summary",
                "Other",
                "--start",
                "2026-01-01T10:00:00Z",
                "--duration",
                "1h",
                "--meet",
            ],
            &t,
        )
        .unwrap();
        assert_ne!(id(&a), id(&c));
    }

    #[tokio::test]
    async fn insert_request_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/calendars/team@x.com/events"))
            .and(query_param("sendUpdates", "all"))
            .and(query_param("conferenceDataVersion", "1"))
            .and(body_partial_json(json!({"summary": "S"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "E1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let args = insert_args(
            &[
                "gwsr",
                "+insert",
                "--calendar-id",
                "team@x.com",
                "--summary",
                "S",
                "--start",
                "2026-01-01T10:00:00Z",
                "--duration",
                "1h",
                "--meet",
            ],
            &tz("UTC", false),
        )
        .unwrap();
        assert_eq!(insert(&api, &args).await.unwrap()["id"], "E1");
    }

    #[tokio::test]
    async fn agenda_paginates_and_fails_loudly_per_calendar() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/calendarList"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
                {"id": "a@x", "summary": "Work"},
                {"id": "b@x", "summary": "Home"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/a@x/events"))
            .and(query_param("pageToken", "p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
                {"id": "2", "summary": "Second", "start": {"dateTime": "2030-01-01T12:00:00Z"}}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/a@x/events"))
            .and(query_param("timeZone", "UTC"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{"id": "1", "summary": "First", "start": {"dateTime": "2030-01-01T09:00:00Z"}}],
                "nextPageToken": "p2"
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/b@x/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
                {"id": "3", "summary": "All day", "start": {"date": "2030-01-01"}}
            ]})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let args = AgendaArgs {
            window: Window::Days(3),
            calendar_ids: vec![],
            calendar_name: None,
        };
        let v = agenda(&api, &args, &tz("UTC", false)).await.unwrap();
        assert_eq!(v["count"], 3);
        let names: Vec<&str> = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["summary"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["All day", "First", "Second"]);

        let only_home = AgendaArgs {
            window: Window::Days(3),
            calendar_ids: vec![],
            calendar_name: Some("home".into()),
        };
        assert_eq!(
            agenda(&api, &only_home, &tz("UTC", false)).await.unwrap()["count"],
            1
        );
    }

    #[tokio::test]
    async fn agenda_surfaces_calendar_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/calendarList"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [{"id": "a@x", "summary": "Work"}]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/a@x/events"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"error": {"code": 403, "message": "denied"}})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let args = AgendaArgs {
            window: Window::Today,
            calendar_ids: vec![],
            calendar_name: None,
        };
        let err = agenda(&api, &args, &tz("UTC", false)).await.unwrap_err();
        assert!(err.to_string().contains("Work"), "{err}");
    }

    /// Every requested --calendar-id must be found: a typo next to a valid ID
    /// fails the command instead of silently shrinking the agenda.
    #[tokio::test]
    async fn agenda_rejects_each_unmatched_calendar_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/calendarList"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
                {"id": "me@x", "summary": "Me", "primary": true},
                {"id": "a@x", "summary": "Work"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/calendars/[^/]+/events$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let run = |ids: &[&str]| AgendaArgs {
            window: Window::Today,
            calendar_ids: ids.iter().map(|s| (*s).to_string()).collect(),
            calendar_name: None,
        };

        // All IDs match (including the "primary" alias): fine.
        agenda(&api, &run(&["primary", "a@x"]), &tz("UTC", false))
            .await
            .unwrap();

        // One valid ID plus one typo: must fail and name the typo.
        let err = agenda(&api, &run(&["a@x", "typo@x"]), &tz("UTC", false))
            .await
            .expect_err("an unmatched --calendar-id must not be silently ignored");
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_VALIDATION, "{err}");
        assert!(err.to_string().contains("typo@x"), "{err}");
    }

    #[tokio::test]
    async fn freebusy_finds_slots_and_rejects_inaccessible_calendars() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/freeBusy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"calendars": {
                "primary": {"busy": [{"start": "2026-01-05T09:00:00Z", "end": "2026-01-05T12:00:00Z"}]},
                "b@x": {"busy": [{"start": "2026-01-05T13:00:00Z", "end": "2026-01-05T14:00:00Z"}]}
            }})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/freeBusy"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"calendars": {
                    "c@x": {"errors": [{"domain": "global", "reason": "notFound"}]}
                }})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let m = parse(&[
            "gwsr",
            "+freebusy",
            "--start",
            "2026-01-05T09:00:00Z",
            "--end",
            "2026-01-05T17:00:00Z",
            "--calendar-id",
            "primary",
            "--calendar-id",
            "b@x",
            "--slot",
            "1h",
        ]);
        let v = freebusy(
            &api,
            m.subcommand_matches("+freebusy").unwrap(),
            &tz("UTC", false),
        )
        .await
        .unwrap();
        assert_eq!(
            v["freeSlots"],
            json!([
                {"start": "2026-01-05T12:00:00+00:00", "end": "2026-01-05T13:00:00+00:00"},
                {"start": "2026-01-05T14:00:00+00:00", "end": "2026-01-05T17:00:00+00:00"}
            ])
        );
        let err = freebusy(
            &api,
            m.subcommand_matches("+freebusy").unwrap(),
            &tz("UTC", false),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("c@x"));
    }

    #[tokio::test]
    async fn update_merges_attendees_and_keeps_length() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events/E1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "start": {"dateTime": "2026-01-05T09:00:00Z"},
                "end": {"dateTime": "2026-01-05T09:45:00Z"},
                "attendees": [{"email": "a@x", "responseStatus": "accepted"}, {"email": "b@x"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/calendars/primary/events/E1"))
            .and(body_partial_json(json!({
                "attendees": [{"email": "a@x", "responseStatus": "accepted"}, {"email": "c@x"}],
                "end": {"dateTime": "2026-01-05T10:45:00+00:00"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "E1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        let m = parse(&[
            "gwsr",
            "+update",
            "--event-id",
            "E1",
            "--start",
            "2026-01-05T10:00:00Z",
            "--add-attendee",
            "c@x",
            "--remove-attendee",
            "B@x",
        ]);
        update(
            &api,
            m.subcommand_matches("+update").unwrap(),
            &tz("UTC", false),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn update_without_changes_is_error() {
        let api = dry_api("");
        let m = parse(&["gwsr", "+update", "--event-id", "E1"]);
        assert!(
            update(
                &api,
                m.subcommand_matches("+update").unwrap(),
                &tz("UTC", false)
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn rsvp_sets_own_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events/E1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "attendees": [{"email": "org@x", "organizer": true}, {"email": "me@x", "self": true}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/calendars/primary/events/E1"))
            .and(body_partial_json(json!({"attendees": [
                {"email": "org@x"}, {"email": "me@x", "responseStatus": "declined", "comment": "busy"}
            ]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "E1"})))
            .expect(1)
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        rsvp(&api, "primary", "E1", "declined", Some("busy"), "all")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rsvp_not_invited_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"attendees": [{"email": "x@x"}]})),
            )
            .mount(&server)
            .await;
        let api = api(&server.uri(), "");
        assert!(
            rsvp(&api, "primary", "E1", "accepted", None, "all")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn delete_plans_request() {
        let api = dry_api("");
        delete(&api, "primary", "E1", "none").await.unwrap();
        let plan = api.planned();
        assert_eq!(plan[0]["method"], "DELETE");
        assert_eq!(plan[0]["query_params"]["sendUpdates"], "none");
    }

    #[test]
    fn agenda_window_flags_are_exclusive() {
        let cmd = CalendarHelper.inject_commands(
            Command::new("gwsr"),
            &crate::discovery::RestDescription::default(),
        );
        assert!(
            cmd.clone()
                .try_get_matches_from(["gwsr", "+agenda", "--today", "--week"])
                .is_err()
        );
        assert!(
            cmd.try_get_matches_from(["gwsr", "+agenda", "--days", "0"])
                .is_err()
        );
    }

    #[test]
    fn agenda_bounds_use_time_zone() {
        let (start, end) = agenda_bounds(&Window::Today, chrono_tz::America::Denver).unwrap();
        let s = start.to_rfc3339();
        assert!(s.ends_with("-07:00") || s.ends_with("-06:00"), "{s}");
        assert_eq!(end - start, chrono::Duration::days(1));
    }
}
