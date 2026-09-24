---
name: gwsr-workflow-meeting-prep
description: "Google Workflow: Prepare for your next meeting: agenda, attendees, and linked docs."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr workflow +meeting-prep --help"
---

# workflow +meeting-prep

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Prepare for your next meeting: agenda, attendees, and linked docs

## Usage

```bash
gwsr workflow +meeting-prep
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar` | — | primary | Calendar ID (default: primary) |
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gwsr workflow +meeting-prep
gwsr workflow +meeting-prep --calendar Work
```

## Tips

- Read-only — never modifies data.
- Shows the next upcoming event with attendees and description.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
