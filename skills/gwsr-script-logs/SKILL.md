---
name: gwsr-script-logs
description: "Google Apps Script: List recent executions of a project."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr script +logs --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# script +logs

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

List recent executions of a project

## Usage

```bash
gwsr script +logs --script-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script-id` | ✓ | — | Apps Script project ID |
| `--function` | — | — | Only executions of this function |
| `--limit` | — | 50 | Maximum executions to list |

## Examples

```bash
gwsr script +logs --script-id SCRIPT_ID
gwsr script +logs --script-id SCRIPT_ID --function main --limit 10
```

## Tips

- Lists executions (function, status, start time, duration). console.log
- output is stored in Cloud Logging, not returned by this API.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-script](../gwsr-script/SKILL.md) — All manage google apps script projects commands
