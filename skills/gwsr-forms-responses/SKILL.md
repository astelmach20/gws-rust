---
name: gwsr-forms-responses
description: "Google Forms: Export all responses of a form as rows (JSON or CSV)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr forms +responses --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# forms +responses

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Export all responses of a form as rows (JSON or CSV)

## Usage

```bash
gwsr forms +responses --form-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--form-id` | ✓ | — | Form ID |
| `--output` | — | — | Write CSV to this path, or '-' for CSV on stdout |
| `--overwrite` | — | — | Replace an existing --output file |

## Examples

```bash
gwsr forms +responses --form-id FORM_ID
gwsr forms +responses --form-id FORM_ID --output responses.csv
```

## Tips

- Read-only. One row per response; one column per question (grid rows get
- their own column). Multiple answers in a cell are joined with '; ';
- file uploads are listed by file name. A repeated question title gets the
- question ID appended, e.g. 'Comments (1a2b3c4d)'. In CSV, cells starting
- with =, +, - or @ get a leading ' so spreadsheets do not run them as
- formulas.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-forms](../gwsr-forms/SKILL.md) — All read and write google forms commands
