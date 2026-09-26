---
name: gwsr-tasks-add
description: "Google Tasks: Add a task."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr tasks +add --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# tasks +add

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Add a task

## Usage

```bash
gwsr tasks +add --title <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--list-id` | — | @default | Task list ID (@default is your default list) |
| `--title` | ✓ | — | Task title |
| `--notes` | — | — | Task notes |
| `--due` | — | — | Due date, YYYY-MM-DD (Google Tasks does not store a time of day) |

## Examples

```bash
gwsr tasks +add --title 'Send invoice'
gwsr tasks +add --title 'File taxes' --due 2026-04-15 --notes 'Use the new form'
```

## Tips

- The Tasks API keeps only the due date; a --due with a time other than
- midnight is rejected rather than silently truncated.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-tasks](../gwsr-tasks/SKILL.md) — All manage task lists and tasks commands
