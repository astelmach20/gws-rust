---
name: gwsr-gmail-unsubscribe
description: "Gmail: Unsubscribe from a mailing list via RFC 8058 one-click (List-Unsubscribe)."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +unsubscribe --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +unsubscribe

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Unsubscribe from a mailing list via RFC 8058 one-click (List-Unsubscribe)

## Usage

```bash
gwsr gmail +unsubscribe --message-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | ID of a message from the mailing list |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr gmail +unsubscribe --message-id 18f1a2b3c4d
gwsr gmail +unsubscribe --message-id 18f1a2b3c4d --dry-run
```

## Tips

- One-click unsubscribe is performed only when the message advertises
- List-Unsubscribe-Post: List-Unsubscribe=One-Click with an https URL and
- Gmail verified its DKIM signature. Otherwise the available unsubscribe
- links are printed and nothing is sent.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
