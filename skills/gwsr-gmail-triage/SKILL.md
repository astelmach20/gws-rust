---
name: gwsr-gmail-triage
description: "Gmail: Summarize unread messages (sender, subject, date)."
metadata:
  version: 0.24.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +triage --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +triage

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Summarize unread messages (sender, subject, date)

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
gwsr gmail +triage --query 'is:unread in:inbox'
gwsr gmail +triage | jq -r '.messages[].subject'
```

## Tips

- Read-only. The default query, is:unread, searches all mail, archived messages included; add in:inbox to limit it to the Inbox. --query replaces the default rather than narrowing it.
- Use +search for full metadata and paging.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
