---
name: gwsr-gmail-triage
description: "Gmail: Show an unread inbox summary (sender, subject, date)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +triage --help"
---

# gmail +triage

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Show an unread inbox summary (sender, subject, date)

## Usage

```bash
gwsr gmail +triage
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--max` | — | 20 | Maximum number of messages to show |
| `--query` | — | is:unread | Gmail search query |
| `--labels` | — | — | Include label IDs in the output |

## Examples

```bash
gwsr gmail +triage
gwsr gmail +triage --max 5 --query 'from:boss'
gwsr gmail +triage --format table
gwsr gmail +triage | jq -r '.messages[].subject'
```

## Tips

- Read-only. Use +search for full metadata and paging.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
