---
name: gws-apps-script-open
version: 1.0.0
description: "Google Apps Script: Open the script editor in your browser."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script +open --help"
---

# apps-script +open

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Open the script editor in your browser

## Usage

```bash
gws apps-script +open
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script` | — | — | Script Project ID (reads .clasp.json if omitted) |

## Examples

```bash
gws script +open --script SCRIPT_ID
gws script +open                        # uses .clasp.json
```

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-apps-script](../gws-apps-script/SKILL.md) — All manage and execute apps script projects commands
