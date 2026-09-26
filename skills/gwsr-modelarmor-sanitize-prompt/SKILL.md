---
name: gwsr-modelarmor-sanitize-prompt
description: "Google Model Armor: Sanitize a user prompt through a Model Armor template."
metadata:
  version: 0.23.0
  openclaw:
    category: "security"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr modelarmor +sanitize-prompt --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# modelarmor +sanitize-prompt

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Sanitize a user prompt through a Model Armor template

## Usage

```bash
gwsr modelarmor +sanitize-prompt --template <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--template` | ✓ | — | Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE) |
| `--text` | — | — | Text content to sanitize |
| `--json` | — | — | Full JSON request body (instead of --text) |

## Examples

```bash
gwsr modelarmor +sanitize-prompt --template projects/P/locations/L/templates/T --text 'user input'
echo 'prompt' | gwsr modelarmor +sanitize-prompt --template ...
```

## Tips

- If neither --text nor --json is given, reads from stdin.
- For outbound safety, use +sanitize-response instead.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-modelarmor](../gwsr-modelarmor/SKILL.md) — All filter user-generated content for safety commands
