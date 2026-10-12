---
name: gwsr-groupssettings
description: "Manage Google Groups settings."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr groupssettings --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# groupssettings (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr groupssettings <resource> <method> [flags]
```

## API Resources

### groups

  - `get` — Gets one resource by id.
  - `patch` — Updates an existing resource. This method supports patch semantics.
  - `update` — Updates an existing resource.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr groupssettings --help

# Inspect a method's required params, types, and defaults
gwsr schema groupssettings.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

