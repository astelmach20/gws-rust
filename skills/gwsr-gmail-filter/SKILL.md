---
name: gwsr-gmail-filter
description: "Gmail: List, create, or delete Gmail filters."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +filter --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +filter

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

List, create, or delete Gmail filters

## Usage

```bash
gwsr gmail +filter
```

## Examples

```bash
gwsr gmail +filter list
gwsr gmail +filter create --from newsletter@example.com --add-label Newsletters --archive
gwsr gmail +filter create --query 'subject:(invoice OR receipt)' --add-label Receipts
gwsr gmail +filter delete --filter-id ANe1Bmj... --yes
```

## Tips

- Requires the gmail.settings.basic scope (gwsr auth login -s gmail).
- Label names must already exist. Deleting a filter always requires --yes.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
