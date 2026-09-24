---
name: gwsr-tasks-list
description: "Google Tasks: List tasks in a task list."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr tasks +list --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# tasks +list

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

List tasks in a task list

## Usage

```bash
gwsr tasks +list
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--list-id` | — | @default | Task list ID (@default is your default list) |
| `--show-completed` | — | — | Include completed tasks |
| `--limit` | — | — | Maximum tasks (default: all) |

## Examples

```bash
gwsr tasks +list
gwsr tasks +list --show-completed --format table
```

## Tips

- Read-only. Fetches every page unless --limit is given.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-tasks](../gwsr-tasks/SKILL.md) — All manage task lists and tasks commands
