---
name: gwsr-gmail-archive
description: "Gmail: Archive messages or threads (remove from Inbox)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +archive --help"
---

# gmail +archive

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Archive messages or threads (remove from Inbox)

## Usage

```bash
gwsr gmail +archive
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | — | — | Message ID to act on (repeatable or comma-separated) |
| `--thread-id` | — | — | Thread ID or Gmail web URL to act on (repeatable or comma-separated) |

## Examples

```bash
gwsr gmail +archive --message-id 18f1a2b3c4d
gwsr gmail +archive --thread-id 18f1a2b3c4d,18f1a2b3c4e
```

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
