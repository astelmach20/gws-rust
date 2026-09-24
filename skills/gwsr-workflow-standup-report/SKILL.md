---
name: gwsr-workflow-standup-report
description: "Google Workflow: Today's meetings and open tasks as a standup summary."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr workflow +standup-report --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# workflow +standup-report

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Today's meetings and open tasks as a standup summary

## Usage

```bash
gwsr workflow +standup-report
```

## Examples

```bash
gwsr workflow +standup-report
gwsr workflow +standup-report --format table
```

## Tips

- Read-only. Combines today's calendar agenda (account time zone) with open tasks.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
