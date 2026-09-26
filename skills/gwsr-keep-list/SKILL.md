---
name: gwsr-keep-list
description: "Google Keep: List notes."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr keep +list --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# keep +list

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

List notes

## Usage

```bash
gwsr keep +list
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--trashed` | — | — | List trashed notes instead |
| `--changed-since` | — | — | Only notes changed after this RFC 3339 time |
| `--limit` | — | — | Maximum notes (default: all) |

## Examples

```bash
gwsr keep +list
gwsr keep +list --changed-since 2026-01-01T00:00:00Z
```

## Tips

- Read-only. The Keep API is only available to Workspace accounts and
- usually requires a service account with domain-wide delegation.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-keep](../gwsr-keep/SKILL.md) — All manage google keep notes commands
