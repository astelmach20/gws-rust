---
name: gws-apps-script-pull
version: 1.0.0
description: "Google Apps Script: Download project files to local directory."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script +pull --help"
---

# apps-script +pull

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Download project files to local directory

## Usage

```bash
gws apps-script +pull
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script` | — | — | Script Project ID (reads .clasp.json if omitted) |
| `--dir` | — | — | Output directory (reads .clasp.json rootDir, or defaults to current dir) |

## Examples

```bash
gws script +pull --script SCRIPT_ID
gws script +pull --script SCRIPT_ID --dir ./src
gws script +pull                        # uses .clasp.json
FILES CREATED:
SERVER_JS  → {name}.gs
HTML       → {name}.html
JSON       → appsscript.json
```

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-apps-script](../gws-apps-script/SKILL.md) — All manage and execute apps script projects commands
