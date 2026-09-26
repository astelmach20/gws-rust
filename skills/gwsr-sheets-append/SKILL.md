---
name: gwsr-sheets-append
description: "Google Sheets: Append rows after the last row of a table."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr sheets +append --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# sheets +append

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Append rows after the last row of a table

## Usage

```bash
gwsr sheets +append --spreadsheet-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet-id` | ✓ | — | Spreadsheet ID |
| `--range` | — | — | Table to append to in A1 notation, e.g. 'Sheet2!A1' (default: A1 of the first sheet) |
| `--values` | — | — | One row as CSV (quote cells containing commas: 'a,"b,c",d') |
| `--json-values` | — | — | JSON array of rows, e.g. '[["a",1],["b",2]]' (a flat array is one row) |
| `--csv-file` | — | — | Import rows from a CSV file, or '-' for stdin |
| `--raw` | — | — | Store input as-is (RAW) instead of parsing it like typed input (USER_ENTERED: formulas, numbers, dates) |

## Examples

```bash
gwsr sheets +append --spreadsheet-id ID --values 'Alice,100,true'
gwsr sheets +append --spreadsheet-id ID --json-values '[["a","b"],["c","d"]]'
gwsr sheets +append --spreadsheet-id ID --range 'Sheet2!A1' --csv-file ./rows.csv
```

## Tips

- Rows are inserted (INSERT_ROWS), never overwriting existing data.
- Input is parsed like typed input unless --raw is given.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-sheets](../gwsr-sheets/SKILL.md) — All read and write spreadsheets commands
