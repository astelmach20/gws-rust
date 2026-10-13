---
name: gwsr-drive-sync
description: "Google Drive: Mirror a Drive folder into a local directory (one-way, download only)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr drive +sync --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +sync

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

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
| `--overwrite` | — | — | Also replace local files that changed since the last sync, or that it did not write (default: keep them and report them under "conflicts") |

## Examples

```bash
gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror
gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --dry-run
gwsr drive +sync --folder-id FOLDER_ID --dir ./mirror --overwrite
```

## Tips

- Recurses into sub-folders. Regular files are downloaded; Google
- Docs/Sheets/Slides/Drawings are exported as docx/xlsx/pptx/pdf.
- What was mirrored is recorded in DIR/.gwsr-sync.json, keyed by Drive file
- ID (Drive version, checksums, modified time; local size, mtime, SHA-256).
- A file is downloaded again only when Drive's copy changed, or another
- Drive item now has its name. A local file that was edited since the last
- sync, or that the sync did not write, is never overwritten without
- --overwrite: it is listed under "conflicts" (reason modifiedLocally or
- untracked) and the command exits 1 with reason syncConflict after printing
- the report. A missing or corrupt state file makes every existing local
- file untracked, except files whose Drive SHA-256 checksum matches. Local
- files that are not in Drive are left untouched (nothing is ever deleted).
- Other Google-native types (Forms, Sites, shortcuts...) are reported as skipped.
- A file the API refuses (download restricted, export too large) is listed
- under "failed" and the rest are still mirrored; the report is printed and
- the command then exits 1 with reason syncPartialFailure.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
