---
name: gwsr-workflow-meeting-prep
description: "Google Workflow: Prepare for your next meeting: agenda, attendees, and links."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr workflow +meeting-prep --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# workflow +meeting-prep

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Prepare for your next meeting: agenda, attendees, and links

## Usage

```bash
gwsr workflow +meeting-prep
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar-id` | — | primary | Calendar ID |

## Examples

```bash
gwsr workflow +meeting-prep
gwsr workflow +meeting-prep --calendar-id team@example.com
```

## Tips

- Read-only. Shows the next upcoming event with attendees and description.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
