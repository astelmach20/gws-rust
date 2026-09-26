---
name: gwsr-chromepolicy
description: "Google Chrome Policy: Manage Chrome policies for users and devices."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr chromepolicy --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# chromepolicy (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr chromepolicy <resource> <method> [flags]
```

## API Resources

### customers

  - `policies` — Operations on the 'policies' resource
  - `policySchemas` — Operations on the 'policySchemas' resource

### media

  - `upload` — Creates an enterprise file from the content provided by user. Returns a public download url for end user.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr chromepolicy --help

# Inspect a method's required params, types, and defaults
gwsr schema chromepolicy.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

