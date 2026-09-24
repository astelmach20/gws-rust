---
name: gwsr-calendar-delete
description: "Google Calendar: Delete an event."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr calendar +delete --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# calendar +delete

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Delete an event

## Usage

```bash
gwsr calendar +delete --event-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | primary | Calendar ID |
| `--event-id` | ✓ | — | Event ID |
| `--send-updates` | — | all | Who gets email notifications |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr calendar +delete --event-id EVENT_ID --yes
gwsr calendar +delete --event-id EVENT_ID --send-updates none --yes
```

## Tips

- Destructive: requires --yes (or a confirmation prompt on a terminal).
- Attendees are notified of the cancellation unless --send-updates none.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-calendar](../gwsr-calendar/SKILL.md) — All manage calendars and events commands
