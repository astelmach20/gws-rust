---
name: gwsr-drive-upload
description: "Google Drive: Upload a local file (resumable, Shared Drive aware)."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr drive +upload --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +upload

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Upload a local file (resumable, Shared Drive aware)

## Usage

```bash
gwsr drive +upload --file <PATH>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file` | ✓ | — | Local file to upload |
| `--folder-id` | — | — | Destination folder ID (My Drive or Shared Drive). Defaults to My Drive root |
| `--name` | — | — | Name in Drive (defaults to the local file name) |
| `--mime-type` | — | — | Content type of the local file (default: detected from the extension) |
| `--convert` | — | — | Convert to the matching Google format (Docs/Sheets/Slides) on import |

## Examples

```bash
gwsr drive +upload --file ./report.pdf
gwsr drive +upload --file ./report.pdf --folder-id FOLDER_ID
gwsr drive +upload --file ./data.csv --name 'Sales Data' --convert
```

## Tips

- Uses the resumable upload protocol in 8 MiB chunks, so large files work.
- Works with Shared Drive folders (supportsAllDrives is always set).
- --convert turns .docx/.csv/.xlsx/.pptx/... into native Google files.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
