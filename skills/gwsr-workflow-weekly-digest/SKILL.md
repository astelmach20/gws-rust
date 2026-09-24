---
name: gwsr-workflow-weekly-digest
description: "Google Workflow: Weekly summary: this week's meetings + unread email count."
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

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Weekly summary: this week's meetings + unread email count

## Usage

```bash
gwsr workflow +weekly-digest
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gwsr workflow +weekly-digest
gwsr workflow +weekly-digest --format table
```

## Tips

- Read-only — never modifies data.
- Combines calendar agenda (week) with gmail triage summary.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
