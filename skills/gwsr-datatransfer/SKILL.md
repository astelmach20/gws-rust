---
name: gwsr-datatransfer
description: "Google Workspace Admin SDK: Transfer user data between accounts (Admin SDK Data Transfer)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr datatransfer --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# datatransfer (datatransfer_v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr datatransfer <resource> <method> [flags]
```

## API Resources

### applications

  - `get` — Retrieves information about an application for the given application ID.
  - `list` — Lists the applications available for data transfer for a customer.

### transfers

  - `get` — Retrieves a data transfer request by its resource ID.
  - `insert` — Inserts a data transfer request. See the [Transfer parameters](https://developers.google.com/workspace/admin/data-transfer/v1/parameters) reference for specific application requirements.
  - `list` — Lists the transfers for a customer by source user, destination user, or status.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr datatransfer --help

# Inspect a method's required params, types, and defaults
gwsr schema datatransfer.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

