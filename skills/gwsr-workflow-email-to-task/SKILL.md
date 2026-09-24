---
name: gwsr-workflow-email-to-task
description: "Google Workflow: Convert a Gmail message into a Google Tasks entry."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr workflow +email-to-task --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# workflow +email-to-task

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Convert a Gmail message into a Google Tasks entry

## Usage

```bash
gwsr workflow +email-to-task --message-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to convert |
| `--tasklist-id` | — | @default | Task list ID |

## Examples

```bash
gwsr workflow +email-to-task --message-id MSG_ID
gwsr workflow +email-to-task --message-id MSG_ID --tasklist-id LIST_ID
```

## Tips

- Uses the email subject as the task title and the snippet as notes.
- Creates a task; preview with --dry-run.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
