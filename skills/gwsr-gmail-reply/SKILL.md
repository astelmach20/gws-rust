---
name: gwsr-gmail-reply
description: "Gmail: Reply to a message (handles threading automatically)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +reply --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +reply

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Reply to a message (handles threading automatically)

## Usage

```bash
gwsr gmail +reply --message-id <ID> --body <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to reply to |
| `--body` | ✓ | — | Reply body (plain text, or HTML with --html) |
| `--from` | — | — | Send-as address to send from (must be configured in Gmail; omit for the default) |
| `--to` | — | — | Additional To email address(es), comma-separated |
| `--attach` | — | — | Attach a file (repeatable) |
| `--cc` | — | — | CC email address(es), comma-separated |
| `--bcc` | — | — | BCC email address(es), comma-separated |
| `--html` | — | — | Treat --body as HTML (a plain-text alternative is generated) |
| `--draft` | — | — | Save as a draft instead of sending |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
gwsr gmail +reply --message-id 18f1a2b3c4d --body '<b>Bold reply</b>' --html
gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Draft reply' --draft
```

## Tips

- Sets In-Reply-To, References, and threadId, and quotes the original message.
- With --html, inline images in the quoted message are preserved via cid: references.
- For reply-all, use +reply-all instead.
- --sanitize screens the subject and body, quoted original included, before sending;
- in block mode a match or a Model Armor failure sends nothing. --dry-run does not call Model Armor.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
