---
name: gwsr-gmail-reply-all
description: "Gmail: Reply to all recipients of a message (handles threading automatically)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +reply-all --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +reply-all

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Reply to all recipients of a message (handles threading automatically)

## Usage

```bash
gwsr gmail +reply-all --message-id <ID> --body <TEXT>
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
| `--remove` | — | — | Exclude recipients from the reply (comma-separated emails) |

## Examples

```bash
gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
gwsr gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com
```

## Tips

- Replies to the sender and all original To/CC recipients, excluding yourself.
- The command fails if no To recipient remains after exclusions and --to additions.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
