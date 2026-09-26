---
name: gwsr-gmail-label
description: "Gmail: Add or remove labels on messages or threads."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +label --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +label

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Add or remove labels on messages or threads

## Usage

```bash
gwsr gmail +label
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--add` | — | — | Label names or IDs to add (repeatable or comma-separated) |
| `--remove` | — | — | Label names or IDs to remove (repeatable or comma-separated) |
| `--message-id` | — | — | Message ID to act on (repeatable or comma-separated) |
| `--thread-id` | — | — | Thread ID or Gmail web URL to act on (repeatable or comma-separated) |

## Examples

```bash
gwsr gmail +label --message-id 18f1a2b3c4d --add Receipts
gwsr gmail +label --message-id ID1,ID2 --add Important --remove UNREAD
gwsr gmail +label --thread-id 'https://mail.google.com/mail/u/0/#inbox/FMfcgz...' --add Follow-up
```

## Tips

- Labels are matched by ID first, then by name (case-insensitive). Unknown labels are an error.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
