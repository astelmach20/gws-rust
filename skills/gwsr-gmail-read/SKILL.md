---
name: gwsr-gmail-read
description: "Gmail: Read a message and extract its body or headers."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +read --help"
---

# gmail +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Read a message and extract its body or headers

## Usage

```bash
gwsr gmail +read --id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--id` | ✓ | — | The Gmail message ID to read |
| `--headers` | — | — | Include headers (From, To, Subject, Date) in the output |
| `--format` | — | text | Output format (text, json) |
| `--html` | — | — | Return HTML body instead of plain text |
| `--dry-run` | — | — | Show the request that would be sent without executing it |

## Examples

```bash
gwsr gmail +read --id 18f1a2b3c4d
gwsr gmail +read --id 18f1a2b3c4d --headers
gwsr gmail +read --id 18f1a2b3c4d --format json | jq '.body'
```

## Tips

- Converts HTML-only messages to plain text automatically.
- Handles multipart/alternative and base64 decoding.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
