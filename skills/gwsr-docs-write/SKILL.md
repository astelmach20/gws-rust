---
name: gwsr-docs-write
description: "Google Docs: Append text to a document."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr docs +write --help"
---

# docs +write

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Append text to a document

## Usage

```bash
gwsr docs +write --document <ID> --text <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document` | ✓ | — | Document ID |
| `--text` | ✓ | — | Text to append (plain text) |

## Examples

```bash
gwsr docs +write --document DOC_ID --text 'Hello, world!'
```

## Tips

- Text is inserted at the end of the document body.
- For rich formatting, use the raw batchUpdate API instead.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-docs](../gwsr-docs/SKILL.md) — All read and write google docs commands
