---
name: gwsr-gmail-read
description: "Gmail: Read a message and print its body and optionally headers."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +read --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Read a message and print its body and optionally headers

## Usage

```bash
gwsr gmail +read --message-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to read |
| `--headers` | — | — | With --body-format text, print From/To/Cc/Subject/Date before the body |
| `--body-format` | — | json | Print the message as a JSON object or as plain text |
| `--html` | — | — | With --body-format text, print the HTML body instead of plain text |

## Examples

```bash
gwsr gmail +read --message-id 18f1a2b3c4d | jq -r '.body_text'
gwsr gmail +read --message-id 18f1a2b3c4d --body-format text --headers
```

## Tips

- Prints the parsed message as JSON by default; --body-format text prints the body.
- HTML-only messages are rendered to plain text automatically.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
