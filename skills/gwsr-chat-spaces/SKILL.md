---
name: gwsr-chat-spaces
description: "Google Chat: List the spaces you are a member of."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr chat +spaces --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# chat +spaces

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

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
