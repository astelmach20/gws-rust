---
name: gwsr-slides-create
description: "Google Slides: Create a new presentation."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr slides +create --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# slides +create

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Create a new presentation

## Usage

```bash
gwsr slides +create --title <TITLE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--title` | ✓ | — | Presentation title |

## Examples

```bash
gwsr slides +create --title 'Q3 review'
```

## Tips

- Prints presentationId and url.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-slides](../gwsr-slides/SKILL.md) — All read and write presentations commands
