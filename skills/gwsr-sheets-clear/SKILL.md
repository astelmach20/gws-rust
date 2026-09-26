---
name: gwsr-sheets-clear
description: "Google Sheets: Clear all values in a range (formatting is kept)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr sheets +clear --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# sheets +clear

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Clear all values in a range (formatting is kept)

## Usage

```bash
gwsr sheets +clear --spreadsheet-id <ID> --range <RANGE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet-id` | ✓ | — | Spreadsheet ID |
| `--range` | ✓ | — | Range to clear, e.g. 'Sheet1!A2:Z' |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr sheets +clear --spreadsheet-id ID --range 'Sheet1!A2:Z' --yes
```

## Tips

- Destructive: requires --yes (or a confirmation prompt on a terminal).

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-sheets](../gwsr-sheets/SKILL.md) — All read and write spreadsheets commands
