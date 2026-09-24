---
name: gwsr-calendar-update
description: "Google Calendar: Change fields of an existing event."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr calendar +update --help"
---

# calendar +update

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Change fields of an existing event

## Usage

```bash
gwsr calendar +update --event-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | primary | Calendar ID |
| `--event-id` | ✓ | — | Event ID |
| `--summary` | — | — | New title |
| `--location` | — | — | New location |
| `--description` | — | — | New description |
| `--add-attendee` | — | — | Invite this email (repeatable) |
| `--remove-attendee` | — | — | Uninvite this email (repeatable) |
| `--timezone` | — | — | IANA time zone for times without a UTC offset (default: your Google account time zone) |
| `--send-updates` | — | all | Who gets email notifications |
| `--start` | — | — | Start: 2026-06-17 (all-day), 2026-06-17T09:00 (in --timezone) or RFC 3339 with offset |
| `--end` | — | — | End, same forms as --start (all-day end dates are exclusive) |
| `--duration` | — | — | Length instead of --end, e.g. 30m, 1h, 1h30m |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr calendar +update --event-id EVENT_ID --summary 'New title'
gwsr calendar +update --event-id EVENT_ID --start 2026-06-17T10:00 --duration 45m
gwsr calendar +update --event-id EVENT_ID --add-attendee carol@example.com --remove-attendee bob@example.com
```

## Tips

- Only the given fields change. Moving --start without --end/--duration keeps
- the event's current length.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
