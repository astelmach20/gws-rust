---
name: gwsr-sheets-write
description: "Google Sheets: Overwrite values in a range (values.update)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr sheets +write --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# sheets +write

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Overwrite values in a range (values.update)

## Usage

```bash
gwsr sheets +write --spreadsheet-id <ID> --range <RANGE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet-id` | ✓ | — | Spreadsheet ID |
| `--range` | ✓ | — | Top-left cell or range to write, e.g. 'Sheet1!B2' |
| `--values` | — | — | One row as CSV (quote cells containing commas: 'a,"b,c",d') |
| `--json-values` | — | — | JSON array of rows, e.g. '[["a",1],["b",2]]' (a flat array is one row) |
| `--csv-file` | — | — | Import rows from a CSV file, or '-' for stdin |
| `--raw` | — | — | Store input as-is (RAW) instead of parsing it like typed input (USER_ENTERED: formulas, numbers, dates) |

## Examples

```bash
gwsr sheets +write --spreadsheet-id ID --range 'Sheet1!B2' --values 'x,y,z'
gwsr sheets +write --spreadsheet-id ID --range 'Sheet1!A1' --json-values '[["Name","Score"],["Ann",9]]'
gwsr sheets +write --spreadsheet-id ID --range 'Import!A1' --csv-file data.csv
```

## Tips

- Existing cells in the written area are overwritten.
- Use +clear first to remove stale data outside the new values.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-sheets](../gwsr-sheets/SKILL.md) — All read and write spreadsheets commands
