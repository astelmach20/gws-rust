---
name: gwsr-chat-read
description: "Google Chat: Read the most recent messages in a space."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr chat +read --help"
---

# chat +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Read the most recent messages in a space

## Usage

```bash
gwsr chat +read --space-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--space-id` | ✓ | — | Space ID, as 'AAAA...' or 'spaces/AAAA...' |
| `--limit` | — | 25 | Number of messages |

## Examples

```bash
gwsr chat +read --space-id spaces/AAAAxxxx
gwsr chat +read --space-id AAAAxxxx --limit 100 --format table
```

## Tips

- Read-only. Newest messages first; the output says when more exist.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-chat](../gwsr-chat/SKILL.md) — All manage chat spaces and messages commands
