---
name: gwsr-calendar-rsvp
description: "Google Calendar: Respond to an event invitation."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr calendar +rsvp --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# calendar +rsvp

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Respond to an event invitation

## Usage

```bash
gwsr calendar +rsvp --event-id <ID> --response <RESPONSE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | primary | Calendar ID |
| `--event-id` | ✓ | — | Event ID |
| `--response` | ✓ | — | Your response |
| `--comment` | — | — | Note to the organizer |
| `--send-updates` | — | all | Who gets email notifications |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr calendar +rsvp --event-id EVENT_ID --response accepted
gwsr calendar +rsvp --event-id EVENT_ID --response declined --comment 'Out that day'
```

## Tips

- Fails if you are not on the event's guest list.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
