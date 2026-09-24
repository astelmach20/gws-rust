---
name: gwsr-docs-write
description: "Google Docs: Append text or Markdown to the end of a document."
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

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Append text or Markdown to the end of a document

## Usage

```bash
gwsr docs +write --document-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document-id` | ✓ | — | Document ID |
| `--text` | — | — | Content to add |
| `--text-file` | — | — | Read the content from a file, or '-' for stdin |
| `--markdown` | — | — | Interpret the content as Markdown and convert it to native Docs formatting |

## Examples

```bash
gwsr docs +write --document-id DOC_ID --text 'Hello, world!'
gwsr docs +write --document-id DOC_ID --markdown --text '# Title
Some **bold** text and a [link](https://example.com).
- item one
- item two'
cat notes.md | gwsr docs +write --document-id DOC_ID --markdown --text-file -
```

## Tips

- Without --markdown the text is appended verbatim to the last paragraph;
- start it with a newline to begin a new paragraph.
- With --markdown the content starts on a new paragraph and headings,
- bold/italic/strikethrough, inline and fenced code, links, block quotes,
- horizontal rules and (nested) bullet/numbered lists become native Docs
- formatting. Tables, images and raw HTML are rejected with an error.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-docs](../gwsr-docs/SKILL.md) — All read and write google docs commands
