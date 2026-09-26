---
name: gwsr-licensing
description: "Google Workspace Enterprise License Manager: Assign and manage product licenses."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr licensing --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# licensing (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr licensing <resource> <method> [flags]
```

## API Resources

### licenseAssignments

  - `delete` — Revoke a license.
  - `get` — Get a specific user's license by product SKU.
  - `insert` — Assign a license.
  - `listForProduct` — List all users assigned licenses for a specific product SKU.
  - `listForProductAndSku` — List all users assigned licenses for a specific product SKU.
  - `patch` — Reassign a user's product SKU with a different SKU in the same product. This method supports patch semantics.
  - `update` — Reassign a user's product SKU with a different SKU in the same product.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr licensing --help

# Inspect a method's required params, types, and defaults
gwsr schema licensing.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

