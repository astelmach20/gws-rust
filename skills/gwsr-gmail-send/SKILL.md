---
name: gwsr-gmail-send
description: "Gmail: Send an email."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +send --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +send

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Send an email

## Usage

```bash
gwsr gmail +send --to <EMAILS> --subject <SUBJECT> --body <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--to` | ✓ | — | Recipient email address(es), comma-separated |
| `--subject` | ✓ | — | Email subject |
| `--body` | ✓ | — | Email body (plain text, or HTML with --html) |
| `--from` | — | — | Send-as address to send from (must be configured in Gmail; omit for the default) |
| `--attach` | — | — | Attach a file (repeatable) |
| `--cc` | — | — | CC email address(es), comma-separated |
| `--bcc` | — | — | BCC email address(es), comma-separated |
| `--html` | — | — | Treat --body as HTML (a plain-text alternative is generated) |
| `--draft` | — | — | Save as a draft instead of sending |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --cc bob@example.com
gwsr gmail +send --to alice@example.com --subject 'Hello' --body '<b>Bold</b> text' --html
gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --from alias@example.com
gwsr gmail +send --to alice@example.com --subject 'Report' --body 'See attached' -a report.pdf
gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --draft
```

## Tips

- Handles RFC 5322 formatting, MIME encoding, and base64 automatically.
- --html sends multipart/alternative with a generated plain-text part.
- Total attachment size limit: 25MB.
- Sends are never retried automatically: a timeout may still have delivered the message.
- With GWSR_REQUIRE_CONFIRM=1, sending requires --yes (drafts do not).

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
