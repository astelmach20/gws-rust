---
name: gwsr-docs-create
description: "Google Docs: Create a new document, optionally with content."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr docs +create --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# docs +create

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Create a new document, optionally with content

## Usage

```bash
gwsr docs +create --title <TITLE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--title` | ✓ | — | Document title |
| `--text` | — | — | Content to add |
| `--text-file` | — | — | Read the content from a file, or '-' for stdin |
| `--markdown` | — | — | Interpret the content as Markdown and convert it to native Docs formatting |

## Examples

```bash
gwsr docs +create --title 'Meeting notes'
gwsr docs +create --title 'Post-mortem' --markdown --text-file ./postmortem.md
```

## Tips

- Prints the new document (documentId, title, url).

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-docs](../gwsr-docs/SKILL.md) — All read and write google docs commands
