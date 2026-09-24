---
name: gwsr-groupssettings
description: "Manage Google Groups settings."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr groupssettings --help"
---

# groupssettings (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

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

