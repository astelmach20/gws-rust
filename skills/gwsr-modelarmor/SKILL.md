---
name: gwsr-modelarmor
description: "Google Model Armor: Filter user-generated content for safety."
metadata:
  version: 0.24.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr modelarmor --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# modelarmor (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr modelarmor +<helper> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+sanitize-prompt`](../gwsr-modelarmor-sanitize-prompt/SKILL.md) | Sanitize a user prompt through a Model Armor template |
| [`+sanitize-response`](../gwsr-modelarmor-sanitize-response/SKILL.md) | Sanitize a model response through a Model Armor template |
| [`+create-template`](../gwsr-modelarmor-create-template/SKILL.md) | Create a new Model Armor template |

## Discovering Commands

Before calling a helper, inspect it:

```bash
# List the helpers
gwsr modelarmor --help

# A helper's flags, examples and tips
gwsr modelarmor +<helper> --help
```

