---
name: gwsr-tasks-lists
description: "Google Tasks: List your task lists."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr tasks +lists --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# tasks +lists

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

List your task lists

## Usage

```bash
gwsr tasks +lists
```

## Examples

```bash
gwsr tasks +lists --format table
```

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-tasks](../gwsr-tasks/SKILL.md) — All manage task lists and tasks commands
