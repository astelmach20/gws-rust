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

//! Time parsing for calendar helpers: event times, durations, and free-slot
//! computation.

use crate::error::GwsError;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};

/// A user-supplied event time.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum When {
    /// `YYYY-MM-DD`: an all-day event date.
    Date(NaiveDate),
    /// RFC 3339 with an explicit offset.
    Instant(DateTime<FixedOffset>),
    /// A wall-clock time with no offset; interpreted in the event time zone.
    Local(NaiveDateTime),
}

const LOCAL_FORMATS: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
];

pub(super) fn parse_when(s: &str, flag: &str) -> Result<When, GwsError> {
    let s = s.trim();
    // Try each accepted form in turn; only when none matches is the input
    // wrong, and that is reported below with every accepted form.
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(When::Date(d));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(When::Instant(dt));
    }
    for f in LOCAL_FORMATS {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Ok(When::Local(dt));
        }
    }
    Err(GwsError::Validation(format!(
        "{flag} '{s}' is not a date (2026-06-17), an RFC 3339 time (2026-06-17T09:00:00-07:00) or a local time (2026-06-17T09:00)"
    )))
}

/// Parse durations like `30m`, `1h`, `1h30m`, `2d`.
pub(super) fn parse_duration(s: &str) -> Result<Duration, GwsError> {
    let err = || {
        GwsError::Validation(format!(
            "Invalid duration '{s}'; use e.g. 30m, 1h, 1h30m, 2d"
        ))
    };
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut any = false;
    for c in s.trim().chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: i64 = num.parse().map_err(|_| err())?;
        num.clear();
        total += match c {
            'd' => Duration::days(n),
            'h' => Duration::hours(n),
            'm' => Duration::minutes(n),
            _ => return Err(err()),
        };
        any = true;
    }
    if !num.is_empty() || !any || total <= Duration::zero() {
        return Err(err());
    }
    Ok(total)
}

impl When {
    /// Add a duration (not valid for all-day dates).
    pub(super) fn plus(&self, d: Duration) -> Result<When, GwsError> {
        match self {
            When::Date(_) => Err(GwsError::Validation(
                "--duration cannot be used with an all-day --start; pass --end YYYY-MM-DD".into(),
            )),
            When::Instant(t) => Ok(When::Instant(*t + d)),
            When::Local(t) => Ok(When::Local(*t + d)),
        }
    }

    /// Calendar API `EventDateTime` JSON. `tz` is the event time zone; it is
    /// required for local times and attached to instants when given.
    pub(super) fn to_event_time(&self, tz: Option<&str>) -> Value {
        match self {
            When::Date(d) => json!({ "date": d.format("%Y-%m-%d").to_string() }),
            When::Instant(t) => {
                let mut v = json!({ "dateTime": t.to_rfc3339() });
                if let Some(tz) = tz {
                    v["timeZone"] = json!(tz);
                }
                v
            }
            When::Local(t) => {
                json!({ "dateTime": t.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": tz })
            }
        }
    }

    pub(super) fn is_local(&self) -> bool {
        matches!(self, When::Local(_))
    }

    /// Resolve to an absolute instant in `tz` (dates are midnight).
    pub(super) fn to_utc(&self, tz: Tz) -> Result<DateTime<Utc>, GwsError> {
        let local = |n: NaiveDateTime| {
            tz.from_local_datetime(&n)
                .earliest()
                .map(|t| t.with_timezone(&Utc))
                .ok_or_else(|| {
                    GwsError::Validation(format!("{n} does not exist in time zone {tz}"))
                })
        };
        match self {
            When::Date(d) => local(d.and_time(NaiveTime::MIN)),
            When::Instant(t) => Ok(t.with_timezone(&Utc)),
            When::Local(n) => local(*n),
        }
    }
}

/// Validate a start/end pair: same kind (all-day vs timed) and end after start.
pub(super) fn check_range(start: &When, end: &When, tz: Tz) -> Result<(), GwsError> {
    match (start, end) {
        (When::Date(s), When::Date(e)) => {
            if e <= s {
                return Err(GwsError::Validation(format!(
                    "All-day --end {e} must be after --start {s} (the end date is exclusive: a one-day event on {s} ends on {})",
                    *s + Duration::days(1)
                )));
            }
        }
        (When::Date(_), _) | (_, When::Date(_)) => {
            return Err(GwsError::Validation(
                "--start and --end must both be dates (all-day) or both be times".into(),
            ));
        }
        _ => {
            if end.to_utc(tz)? <= start.to_utc(tz)? {
                return Err(GwsError::Validation("--end must be after --start".into()));
            }
        }
    }
    Ok(())
}

/// Working hours `HH:MM-HH:MM`.
pub(super) fn parse_working_hours(s: &str) -> Result<(NaiveTime, NaiveTime), GwsError> {
    let err = || {
        GwsError::Validation(format!(
            "Invalid --working-hours '{s}'; use e.g. 09:00-17:00"
        ))
    };
    let (a, b) = s.split_once('-').ok_or_else(err)?;
    let a = NaiveTime::parse_from_str(a.trim(), "%H:%M").map_err(|_| err())?;
    let b = NaiveTime::parse_from_str(b.trim(), "%H:%M").map_err(|_| err())?;
    if b <= a {
        return Err(err());
    }
    Ok((a, b))
}

pub(super) type Interval = (DateTime<Utc>, DateTime<Utc>);

/// Free intervals of at least `min` inside `[from, to)` that avoid `busy` and,
/// when given, fall within daily working hours in `tz`.
pub(super) fn free_slots(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    mut busy: Vec<Interval>,
    min: Duration,
    working: Option<(NaiveTime, NaiveTime)>,
    tz: Tz,
) -> Vec<Interval> {
    busy.sort();
    let mut gaps = Vec::new();
    let mut cursor = from;
    for (s, e) in busy {
        if s > cursor {
            gaps.push((cursor, s.min(to)));
        }
        cursor = cursor.max(e);
        if cursor >= to {
            break;
        }
    }
    if cursor < to {
        gaps.push((cursor, to));
    }
    let windows: Vec<Interval> = match working {
        None => gaps,
        Some((ws, we)) => {
            let mut out = Vec::new();
            for (gs, ge) in gaps {
                let mut day = gs.with_timezone(&tz).date_naive();
                let last = ge.with_timezone(&tz).date_naive();
                while day <= last {
                    let open = tz.from_local_datetime(&day.and_time(ws)).earliest();
                    let close = tz.from_local_datetime(&day.and_time(we)).earliest();
                    if let (Some(o), Some(c)) = (open, close) {
                        let s = gs.max(o.with_timezone(&Utc));
                        let e = ge.min(c.with_timezone(&Utc));
                        if e > s {
                            out.push((s, e));
                        }
                    }
                    let Some(next) = day.succ_opt() else { break };
                    day = next;
                }
            }
            out
        }
    };
    windows
        .into_iter()
        .filter(|(s, e)| *e - *s >= min)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        assert!(matches!(
            parse_when("2026-06-17", "--start").unwrap(),
            When::Date(_)
        ));
        assert!(matches!(
            parse_when("2026-06-17T09:00:00-07:00", "--start").unwrap(),
            When::Instant(_)
        ));
        assert!(matches!(
            parse_when("2026-06-17T09:00", "--start").unwrap(),
            When::Local(_)
        ));
        assert!(matches!(
            parse_when("2026-06-17 09:00:00", "--start").unwrap(),
            When::Local(_)
        ));
        assert!(parse_when("tomorrow", "--start").is_err());
    }

    #[test]
    fn local_time_carries_time_zone() {
        let w = parse_when("2026-03-18T14:00:00", "--start").unwrap();
        assert_eq!(
            w.to_event_time(Some("America/Denver")),
            json!({"dateTime": "2026-03-18T14:00:00", "timeZone": "America/Denver"})
        );
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30m").unwrap(), Duration::minutes(30));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("2d").unwrap(), Duration::days(2));
        for bad in ["", "30", "m", "1x", "0m"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn range_checks() {
        let tz: Tz = "UTC".parse().unwrap();
        let d1 = parse_when("2026-01-01", "s").unwrap();
        let t1 = parse_when("2026-01-01T10:00:00Z", "s").unwrap();
        let t0 = parse_when("2026-01-01T09:00:00Z", "s").unwrap();
        assert!(check_range(&d1, &d1, tz).is_err());
        assert!(check_range(&d1, &t1, tz).is_err());
        assert!(check_range(&t1, &t0, tz).is_err());
        assert!(check_range(&t0, &t1, tz).is_ok());
    }

    #[test]
    fn slots_respect_busy_and_working_hours() {
        let tz: Tz = "UTC".parse().unwrap();
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let busy = vec![
            (t("2026-01-05T10:00:00Z"), t("2026-01-05T11:00:00Z")),
            (t("2026-01-05T10:30:00Z"), t("2026-01-05T12:00:00Z")),
        ];
        let slots = free_slots(
            t("2026-01-05T00:00:00Z"),
            t("2026-01-06T00:00:00Z"),
            busy,
            Duration::minutes(60),
            Some(parse_working_hours("09:00-17:00").unwrap()),
            tz,
        );
        assert_eq!(
            slots,
            vec![
                (t("2026-01-05T09:00:00Z"), t("2026-01-05T10:00:00Z")),
                (t("2026-01-05T12:00:00Z"), t("2026-01-05T17:00:00Z")),
            ]
        );
        // A 60-minute window is dropped when 61 minutes are required.
        let slots = free_slots(
            t("2026-01-05T09:00:00Z"),
            t("2026-01-05T10:00:00Z"),
            vec![],
            Duration::minutes(61),
            None,
            tz,
        );
        assert!(slots.is_empty());
    }
}
