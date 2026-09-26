---
name: gwsr-docs-replace
description: "Google Docs: Find and replace text throughout a document."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr docs +replace --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# docs +replace

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Find and replace text throughout a document

## Usage

```bash
gwsr docs +replace --document-id <ID> --find <TEXT> --replace-with <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document-id` | ✓ | — | Document ID |
| `--find` | ✓ | — | Text to find |
| `--replace-with` | ✓ | — | Replacement text (may be empty to delete matches) |
| `--match-case` | — | — | Case-sensitive matching |

## Examples

```bash
gwsr docs +replace --document-id DOC_ID --find '{{name}}' --replace-with 'Alice'
gwsr docs +replace --document-id DOC_ID --find 'DRAFT' --replace-with '' --match-case
```

## Tips

- Prints occurrencesChanged; 0 means nothing matched.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-docs](../gwsr-docs/SKILL.md) — All read and write google docs commands
