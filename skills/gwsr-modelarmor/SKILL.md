---
name: gwsr-modelarmor
description: "Google Model Armor: Filter user-generated content for safety."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr modelarmor --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# modelarmor (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr modelarmor <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+sanitize-prompt`](../gwsr-modelarmor-sanitize-prompt/SKILL.md) | Sanitize a user prompt through a Model Armor template |
| [`+sanitize-response`](../gwsr-modelarmor-sanitize-response/SKILL.md) | Sanitize a model response through a Model Armor template |
| [`+create-template`](../gwsr-modelarmor-create-template/SKILL.md) | Create a new Model Armor template |

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr modelarmor --help

# Inspect a method's required params, types, and defaults
gwsr schema modelarmor.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

