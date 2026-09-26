---
name: gwsr-gmail-resolve-url
description: "Gmail: Resolve a Gmail web URL (or its FMfcg... token) to an API thread or message ID."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr gmail +resolve-url --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +resolve-url

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Resolve a Gmail web URL (or its FMfcg... token) to an API thread or message ID

## Usage

```bash
gwsr gmail +resolve-url --url <URL>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--url` | ✓ | — | Gmail web URL, or the ID token at the end of it |
| `--no-verify` | — | — | Decode offline only; do not confirm the ID exists via the API |

## Examples

```bash
gwsr gmail +resolve-url --url 'https://mail.google.com/mail/u/0/#inbox/FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW'
gwsr gmail +resolve-url --url FMfcgzQgLjNPlfJCVRfnNkPGkLhWClCW --no-verify
```

## Tips

- --thread-id on +label/+archive/+trash also accepts Gmail web URLs.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
