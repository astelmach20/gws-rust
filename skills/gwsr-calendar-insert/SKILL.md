---
name: gwsr-calendar-insert
description: "Google Calendar: Create a new event."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr calendar +insert --help"
---

# calendar +insert

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Create a new event

## Usage

```bash
gwsr calendar +insert --summary <TEXT> --start <TIME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | primary | Calendar ID |
| `--summary` | ✓ | — | Event title |
| `--location` | — | — | Event location |
| `--description` | — | — | Event description |
| `--attendee` | — | — | Attendee email (repeatable) |
| `--meet` | — | — | Add a Google Meet link |
| `--timezone` | — | — | IANA time zone for times without a UTC offset (default: your Google account time zone) |
| `--send-updates` | — | all | Who gets email notifications |
| `--start` | ✓ | — | Start: 2026-06-17 (all-day), 2026-06-17T09:00 (in --timezone) or RFC 3339 with offset |
| `--end` | — | — | End, same forms as --start (all-day end dates are exclusive) |
| `--duration` | — | — | Length instead of --end, e.g. 30m, 1h, 1h30m |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr calendar +insert --summary 'Standup' --start '2026-06-17T09:00' --duration 30m
gwsr calendar +insert --summary 'Review' --start '2026-06-17T09:00:00-07:00' --end '2026-06-17T10:00:00-07:00' --attendee alice@example.com --meet
gwsr calendar +insert --summary 'Offsite' --start 2026-06-17 --end 2026-06-19
```

## Tips

- Times without an offset use --timezone, else your account time zone.
- A date-only --start creates an all-day event (--end defaults to the next day).
- Timed events need --end or --duration.
- Invitations are emailed to attendees unless --send-updates none.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
