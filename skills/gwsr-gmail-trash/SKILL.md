---
name: gwsr-gmail-trash
description: "Gmail: Move messages or threads to Trash."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +trash --help"
---

# gmail +trash

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Move messages or threads to Trash

## Usage

```bash
gwsr gmail +trash
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | — | — | Message ID to act on (repeatable or comma-separated) |
| `--thread-id` | — | — | Thread ID or Gmail web URL to act on (repeatable or comma-separated) |

## Examples

```bash
gwsr gmail +trash --message-id 18f1a2b3c4d
gwsr gmail +trash --thread-id 18f1a2b3c4d
```

## Tips

- Trashed items can be restored from Gmail's Trash for 30 days.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
