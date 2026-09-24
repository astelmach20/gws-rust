---
name: gwsr-workflow-weekly-digest
description: "Google Workflow: Weekly summary: the next 7 days of meetings and your unread email count."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr workflow +weekly-digest --help"
---

# workflow +weekly-digest

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Weekly summary: the next 7 days of meetings and your unread email count

## Usage

```bash
gwsr workflow +weekly-digest
```

## Examples

```bash
gwsr workflow +weekly-digest
gwsr workflow +weekly-digest --format table
```

## Tips

- Read-only. The unread count is Gmail's estimate for is:unread.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
