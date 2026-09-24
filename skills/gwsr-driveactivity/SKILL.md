---
name: gwsr-driveactivity
description: "Google Drive Activity: Query activity on Drive files and folders."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr driveactivity --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# driveactivity (v2)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr driveactivity <resource> <method> [flags]
```

## API Resources

### activity

  - `query` — Query past activity in Google Drive.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr driveactivity --help

# Inspect a method's required params, types, and defaults
gwsr schema driveactivity.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

