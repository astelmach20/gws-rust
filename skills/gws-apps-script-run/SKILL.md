---
name: gws-apps-script-run
version: 1.0.0
description: "Google Apps Script: Execute a function in the script."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script +run --help"
---

# apps-script +run

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Execute a function in the script

## Usage

```bash
gws apps-script +run --function <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script` | — | — | Script Project ID (reads .clasp.json if omitted) |
| `--function` | ✓ | — | Function name to execute |
| `--dev-mode` | — | — | Run the script in dev mode (HEAD deployment) |

## Examples

```bash
gws script +run --script SCRIPT_ID --function main
gws script +run --function main         # uses .clasp.json
gws script +run --function main --dev-mode
SETUP REQUIREMENTS:
1. Auth with cloud-platform scope: gws auth login
2. Link the script to your OAuth client's GCP project:
Open the script editor (gws apps-script +open) → Project Settings →
Change GCP project → enter your project number.
3. Add to appsscript.json: "executionApi": {"access": "MYSELF"}
```

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-apps-script](../gws-apps-script/SKILL.md) — All manage and execute apps script projects commands
