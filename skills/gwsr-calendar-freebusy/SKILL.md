---
name: gwsr-calendar-freebusy
description: "Google Calendar: Show busy times and find free slots."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr calendar +freebusy --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# calendar +freebusy

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Show busy times and find free slots

## Usage

```bash
gwsr calendar +freebusy --start <TIME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | — | Calendar or person email to check (repeatable; default: primary) |
| `--slot` | — | — | Also list common free slots at least this long, e.g. 30m |
| `--working-hours` | — | — | Restrict free slots to these daily hours, e.g. 09:00-17:00 |
| `--timezone` | — | — | IANA time zone for times without a UTC offset (default: your Google account time zone) |
| `--start` | ✓ | — | Start: 2026-06-17 (all-day), 2026-06-17T09:00 (in --timezone) or RFC 3339 with offset |
| `--end` | — | — | End, same forms as --start (all-day end dates are exclusive) |
| `--duration` | — | — | Length instead of --end, e.g. 30m, 1h, 1h30m |

## Examples

```bash
gwsr calendar +freebusy --start 2026-06-17 --end 2026-06-18
gwsr calendar +freebusy --start 2026-06-17T09:00 --duration 8h --calendar-id alice@example.com --calendar-id bob@example.com --slot 30m
gwsr calendar +freebusy --start 2026-06-15 --end 2026-06-20 --calendar-id primary --calendar-id alice@example.com --slot 1h --working-hours 09:00-17:00
```

## Tips

- Read-only. A calendar you cannot see fails the command (no silent gaps).
- Slot times are reported in --timezone (default: account time zone).

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
