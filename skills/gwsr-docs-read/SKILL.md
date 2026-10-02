---
name: gwsr-docs-read
description: "Google Docs: Read a document as plain text or Markdown."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr docs +read --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# docs +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Read a document as plain text or Markdown

## Usage

```bash
gwsr docs +read --document-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document-id` | ✓ | — | Document ID |
| `--body-format` | — | markdown | How to render the body: markdown, text, or raw (the Docs API document JSON) |
| `--output` | — | — | Write the rendered body to this file, or '-' for raw text on stdout |
| `--overwrite` | — | — | Replace an existing --output file |

## Examples

```bash
gwsr docs +read --document-id DOC_ID
gwsr docs +read --document-id DOC_ID --body-format text --output doc.txt
gwsr docs +read --document-id DOC_ID --output - | less
```

## Tips

- Read-only. Prints JSON {documentId, title, bodyFormat, body} by default.
- Headings, lists, bold/italic/strikethrough, inline code, links and tables
- are rendered; images appear as [image] placeholders.
- Only the first tab of multi-tab documents is read.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-docs](../gwsr-docs/SKILL.md) — All read and write google docs commands
