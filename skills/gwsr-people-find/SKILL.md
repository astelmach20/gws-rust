---
name: gwsr-people-find
description: "Google People: Find people in your contacts or your organization's directory."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr people +find --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# people +find

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Find people in your contacts or your organization's directory

## Usage

```bash
gwsr people +find --query <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--query` | ✓ | — | Name, email or phone prefix to search for |
| `--directory` | — | — | Search the Workspace directory instead of your contacts |
| `--limit` | — | 30 | Maximum results (contacts: at most 30) |

## Examples

```bash
gwsr people +find --query ann
gwsr people +find --query 'ann@example.com' --directory --format table
```

## Tips

- Read-only. Contact search matches name/email/phone prefixes.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-people](../gwsr-people/SKILL.md) — All manage contacts and profiles commands
