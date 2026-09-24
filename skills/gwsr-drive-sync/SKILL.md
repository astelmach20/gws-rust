---
name: gwsr-drive-sync
description: "Google Drive: Mirror a Drive folder into a local directory (one-way, download only)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr drive +sync --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +sync

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Mirror a Drive folder into a local directory (one-way, download only)

## Usage

```bash
gwsr drive +sync --folder-id <ID> --dir <DIR>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--folder-id` | ✓ | — | Drive folder to mirror |
| `--dir` | ✓ | — | Local directory to mirror into (created if missing) |

## Examples

```bash
gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror
gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --dry-run
```

## Tips

- Recurses into sub-folders. Regular files are downloaded; Google
- Docs/Sheets/Slides/Drawings are exported as docx/xlsx/pptx/pdf.
- Local files are replaced only when the Drive copy is newer; local files
- that are not in Drive are left untouched (nothing is ever deleted).
- Other Google-native types (Forms, Sites, shortcuts...) are reported as skipped.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
