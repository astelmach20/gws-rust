---
name: gwsr-chat-spaces
description: "Google Chat: List the spaces you are a member of."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr chat +spaces --help"
---

# chat +spaces

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

List the spaces you are a member of

## Usage

```bash
gwsr chat +spaces
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--type` | — | — | Only this kind of space |

## Examples

```bash
gwsr chat +spaces
gwsr chat +spaces --type space --format table
```

## Tips

- Read-only. All pages are fetched.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-chat](../gwsr-chat/SKILL.md) — All manage chat spaces and messages commands
