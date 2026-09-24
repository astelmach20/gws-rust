---
name: gwsr-script-push
description: "Google Apps Script: Replace a project's files with local files."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr script +push --help"
---

# script +push

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Replace a project's files with local files

## Usage

```bash
gwsr script +push --script-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script-id` | ✓ | — | Apps Script project ID |
| `--dir` | — | . | Directory with the script files |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr script +push --script-id SCRIPT_ID --yes
gwsr script +push --script-id SCRIPT_ID --dir ./src --yes
```

## Tips

- Uploads .gs/.js (server code), .html and appsscript.json (required).
- Files in sub-directories keep their path (e.g. lib/util.gs -> lib/util).
- Hidden files/directories, node_modules and symlinks are skipped.
- Destructive: this REPLACES ALL files in the project, so it requires --yes
- (or a confirmation prompt on a terminal).

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-script](../gwsr-script/SKILL.md) — All manage google apps script projects commands
