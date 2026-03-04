---
name: gws-apps-script-logs
version: 1.0.0
description: "Google Apps Script: View execution logs for the script."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script +logs --help"
---

# apps-script +logs

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

View execution logs for the script

## Usage

```bash
gws apps-script +logs
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script` | — | — | Script Project ID (reads .clasp.json if omitted) |

## Examples

```bash
gws script +logs --script SCRIPT_ID
gws script +logs                        # uses .clasp.json
```

## Tips

- Shows recent script executions and their status.
- Use --format table for a readable summary.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-apps-script](../gws-apps-script/SKILL.md) — All manage and execute apps script projects commands
