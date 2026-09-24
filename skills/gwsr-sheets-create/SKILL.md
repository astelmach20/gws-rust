---
name: gwsr-sheets-create
description: "Google Sheets: Create a new spreadsheet."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr sheets +create --help"
---

# sheets +create

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Create a new spreadsheet

## Usage

```bash
gwsr sheets +create --title <TITLE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--title` | ✓ | — | Spreadsheet title |
| `--sheet` | — | — | Sheet (tab) name; repeat for several (default: one 'Sheet1') |

## Examples

```bash
gwsr sheets +create --title 'Budget 2026'
gwsr sheets +create --title 'Tracker' --sheet Tasks --sheet Archive
```

## Tips

- Prints spreadsheetId and url.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-sheets](../gwsr-sheets/SKILL.md) — All read and write spreadsheets commands
