---
name: gwsr-drive-export
description: "Google Drive: Export a Google Doc/Sheet/Slides/Drawing to another format."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr drive +export --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +export

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Export a Google Doc/Sheet/Slides/Drawing to another format

## Usage

```bash
gwsr drive +export --file-id <ID> --to <FORMAT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID |
| `--to` | ✓ | — | Target format |
| `--output` | — | — | Local path to write, or '-' for stdout. Defaults to the Drive file name |
| `--overwrite` | — | — | Replace an existing local file |

## Examples

```bash
gwsr drive +export --file-id DOC_ID --to pdf
gwsr drive +export --file-id DOC_ID --to md --output -
gwsr drive +export --file-id SHEET_ID --to csv --output sheet1.csv
```

## Tips

- Docs: pdf docx odt rtf txt md html epub
- Sheets: pdf xlsx ods csv tsv html (csv/tsv export the first sheet)
- Slides: pdf pptx odp txt
- Drawings: pdf png jpg svg
- The Drive export endpoint is limited to 10 MB of output.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
