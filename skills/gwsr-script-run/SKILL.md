---
name: gwsr-script-run
description: "Google Apps Script: Run a function in a deployed Apps Script project."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr script +run --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# script +run

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Run a function in a deployed Apps Script project

## Usage

```bash
gwsr script +run --script-id <ID> --function <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script-id` | ✓ | — | Apps Script project ID |
| `--function` | ✓ | — | Function name |
| `--args` | — | — | Function arguments as a JSON array, e.g. '["a", 2]' |
| `--dev-mode` | — | — | Run the most recently saved code instead of the deployed version (owner only) |
| `--scope` | — | — | OAuth scope the function needs (repeatable; default: the scopes listed by the Execution API) |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr script +run --script-id SCRIPT_ID --function main
gwsr script +run --script-id SCRIPT_ID --function add --args '[1, 2]' --dev-mode
```

## Tips

- The project must be deployed as an API executable and share a Cloud
- project with your OAuth client. Prints the function's return value.
- A script error fails the command with the script's message and stack.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-script](../gwsr-script/SKILL.md) — All manage google apps script projects commands
