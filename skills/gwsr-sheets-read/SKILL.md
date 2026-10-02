---
name: gwsr-sheets-read
description: "Google Sheets: Read values from a range, optionally exporting CSV."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr sheets +read --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# sheets +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Read values from a range, optionally exporting CSV

## Usage

```bash
gwsr sheets +read --spreadsheet-id <ID> --range <RANGE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet-id` | ✓ | — | Spreadsheet ID |
| `--range` | ✓ | — | Range to read, e.g. 'Sheet1!A1:D10' or 'Sheet1' |
| `--output` | — | — | Write the values as CSV to this path, or '-' for stdout |
| `--overwrite` | — | — | Replace an existing --output file |

## Examples

```bash
gwsr sheets +read --spreadsheet-id ID --range 'Sheet1!A1:D10'
gwsr sheets +read --spreadsheet-id ID --range Sheet1 --output sheet1.csv
gwsr sheets +read --spreadsheet-id ID --range Sheet1 --output -
```

## Tips

- Read-only. Values are the formatted strings shown in the UI.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-sheets](../gwsr-sheets/SKILL.md) — All read and write spreadsheets commands
