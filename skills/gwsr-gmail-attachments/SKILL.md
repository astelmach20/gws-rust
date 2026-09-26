---
name: gwsr-gmail-attachments
description: "Gmail: Download a message's attachments as decoded files."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +attachments --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +attachments

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Download a message's attachments as decoded files

## Usage

```bash
gwsr gmail +attachments --message-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID whose attachments to download |
| `--output-dir` | — | . | Directory to write files into |
| `--include-inline` | — | — | Also save inline images |
| `--overwrite` | — | — | Overwrite existing files instead of failing |

## Examples

```bash
gwsr gmail +attachments --message-id 18f1a2b3c4d
gwsr gmail +attachments --message-id 18f1a2b3c4d --output-dir ./downloads --include-inline
```

## Tips

- Filenames come from the sender and are sanitized; duplicates get a numeric suffix.
- Existing files are never overwritten unless --overwrite is given.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
