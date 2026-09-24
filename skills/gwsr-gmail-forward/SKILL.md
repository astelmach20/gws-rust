---
name: gwsr-gmail-forward
description: "Gmail: Forward a message to new recipients."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +forward --help"
---

# gmail +forward

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Forward a message to new recipients

## Usage

```bash
gwsr gmail +forward --message-id <ID> --to <EMAILS>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to forward |
| `--to` | ✓ | — | Recipient email address(es), comma-separated |
| `--from` | — | — | Send-as address to send from (must be configured in Gmail; omit for the default) |
| `--body` | — | — | Note to include above the forwarded message (plain text, or HTML with --html) |
| `--no-original-attachments` | — | — | Do not include the original message's file attachments |
| `--attach` | — | — | Attach a file (repeatable) |
| `--cc` | — | — | CC email address(es), comma-separated |
| `--bcc` | — | — | BCC email address(es), comma-separated |
| `--html` | — | — | Treat --body as HTML (a plain-text alternative is generated) |
| `--draft` | — | — | Save as a draft instead of sending |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
gwsr gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --no-original-attachments
```

## Tips

- Original attachments are included by default (matching Gmail web).
- In plain-text mode, inline images are not included (matching Gmail web).
- Combined size of original and added attachments is limited to 25MB.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
