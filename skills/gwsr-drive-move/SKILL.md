---
name: gwsr-drive-move
description: "Google Drive: Move a file or folder to another folder."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr drive +move --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +move

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Move a file or folder to another folder

## Usage

```bash
gwsr drive +move --file-id <ID> --folder-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID |
| `--folder-id` | ✓ | — | Destination folder ID |

## Examples

```bash
gwsr drive +move --file-id FILE_ID --folder-id FOLDER_ID
```

## Tips

- Removes all current parents and adds the destination folder.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
