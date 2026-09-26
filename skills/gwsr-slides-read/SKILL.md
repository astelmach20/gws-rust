---
name: gwsr-slides-read
description: "Google Slides: Extract the text and speaker notes of every slide."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr slides +read --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# slides +read

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Extract the text and speaker notes of every slide

## Usage

```bash
gwsr slides +read --presentation-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--presentation-id` | ✓ | — | Presentation ID |

## Examples

```bash
gwsr slides +read --presentation-id PRES_ID
gwsr slides +read --presentation-id PRES_ID --format yaml
```

## Tips

- Read-only. Text from shapes, tables and groups is returned per slide,
- in page order, with the speaker notes.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-slides](../gwsr-slides/SKILL.md) — All read and write presentations commands
