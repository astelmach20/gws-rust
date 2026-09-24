---
name: gwsr-classroom-courses
description: "Google Classroom: List your courses."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr classroom +courses --help"
---

# classroom +courses

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

List your courses

## Usage

```bash
gwsr classroom +courses
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--role` | — | — | Only courses where you are a teacher or a student |
| `--state` | — | — | Only courses in this state (default: all) |
| `--limit` | — | — | Maximum courses (default: all) |

## Examples

```bash
gwsr classroom +courses
gwsr classroom +courses --role teacher --state active --format table
```

## Tips

- Read-only. Fetches every page unless --limit is given.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-classroom](../gwsr-classroom/SKILL.md) — All manage classes, rosters, and coursework commands
