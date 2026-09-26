---
name: gwsr-drive-download
description: "Google Drive: Download a (non-Google-native) file's content."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr drive +download --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +download

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Download a (non-Google-native) file's content

## Usage

```bash
gwsr drive +download --file-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID |
| `--output` | — | — | Local path to write, or '-' for stdout. Defaults to the Drive file name |
| `--overwrite` | — | — | Replace an existing local file |

## Examples

```bash
gwsr drive +download --file-id FILE_ID
gwsr drive +download --file-id FILE_ID --output ./copy.pdf
gwsr drive +download --file-id FILE_ID --output - | sha256sum
```

## Tips

- Google Docs/Sheets/Slides have no binary content; use +export instead.
- Existing local files are never replaced unless --overwrite is given.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
