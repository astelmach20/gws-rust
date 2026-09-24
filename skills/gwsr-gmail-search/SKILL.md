---
name: gwsr-gmail-search
description: "Gmail: Search messages and return full metadata, with pagination."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +search --help"
---

# gmail +search

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Search messages and return full metadata, with pagination

## Usage

```bash
gwsr gmail +search
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--query` | — | — | Gmail search query (same syntax as the Gmail search box) |
| `--max` | — | 25 | Maximum number of messages to return across pages |
| `--page-token` | — | — | Resume from a nextPageToken returned by a previous search |
| `--include-spam-trash` | — | — | Include messages from Spam and Trash |

## Examples

```bash
gwsr gmail +search --query 'from:alice has:attachment newer_than:7d'
gwsr gmail +search --query 'label:receipts' --max 200 --format table
gwsr gmail +search --query 'is:starred' --page-token TOKEN
```

## Tips

- Output includes id, threadId, labelIds, snippet, date, from, to, cc, subject, sizeEstimate.
- When more results exist than --max, the output includes nextPageToken.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
