---
name: gwsr-script-pull
description: "Google Apps Script: Download a project's files into a local directory."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr script +pull --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# script +pull

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Download a project's files into a local directory

## Usage

```bash
gwsr script +pull --script-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script-id` | ✓ | — | Apps Script project ID |
| `--dir` | — | . | Destination directory (created if missing) |
| `--overwrite` | — | — | Replace existing local files |

## Examples

```bash
gwsr script +pull --script-id SCRIPT_ID --dir ./src
gwsr script +pull --script-id SCRIPT_ID --dir ./src --overwrite
```

## Tips

- Server code is written as .gs, HTML as .html, the manifest as appsscript.json.
- Nothing is written if any target file exists and --overwrite is not given.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-script](../gwsr-script/SKILL.md) — All manage google apps script projects commands
