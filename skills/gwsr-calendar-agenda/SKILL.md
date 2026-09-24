---
name: gwsr-calendar-agenda
description: "Google Calendar: Show upcoming events across calendars."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr calendar +agenda --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# calendar +agenda

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Show upcoming events across calendars

## Usage

```bash
gwsr calendar +agenda
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--today` | — | — | Today's events |
| `--tomorrow` | — | — | Tomorrow's events |
| `--week` | — | — | The next 7 days |
| `--days` | — | — | The next N days (default: 1) |
| `--calendar-id` | — | — | Only this calendar (repeatable) |
| `--calendar-name` | — | — | Only calendars whose name contains TEXT |
| `--timezone` | — | — | IANA time zone for times without a UTC offset (default: your Google account time zone) |

## Examples

```bash
gwsr calendar +agenda
gwsr calendar +agenda --today
gwsr calendar +agenda --week --format table
gwsr calendar +agenda --days 3 --calendar-name Work
gwsr calendar +agenda --today --timezone America/New_York
```

## Tips

- Read-only. Every page of every selected calendar is fetched (no truncation);
- a calendar that cannot be read fails the command with its name.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
