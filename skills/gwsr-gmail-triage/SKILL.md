---
name: gwsr-gmail-triage
description: "Gmail: Show unread inbox summary (sender, subject, date)."
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

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Show unread inbox summary (sender, subject, date)

## Usage

```bash
gwsr gmail +triage
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--max` | — | 20 | Maximum messages to show (default: 20) |
| `--query` | — | — | Gmail search query (default: is:unread) |
| `--labels` | — | — | Include label names in output |

## Examples

```bash
gwsr gmail +triage
gwsr gmail +triage --max 5 --query 'from:boss'
gwsr gmail +triage --format json | jq '.[].subject'
gwsr gmail +triage --labels
```

## Tips

- Read-only — never modifies your mailbox.
- Defaults to table output format.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
