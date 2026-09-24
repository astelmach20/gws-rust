---
name: gwsr-modelarmor-sanitize-response
description: "Google Model Armor: Sanitize a model response through a Model Armor template."
metadata:
  version: 0.22.5
  openclaw:
    category: "security"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr modelarmor +sanitize-response --help"
---

# modelarmor +sanitize-response

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Sanitize a model response through a Model Armor template

## Usage

```bash
gwsr modelarmor +sanitize-response --template <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--template` | ✓ | — | Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE) |
| `--text` | — | — | Text content to sanitize |
| `--json` | — | — | Full JSON request body (instead of --text) |

## Examples

```bash
gwsr modelarmor +sanitize-response --template projects/P/locations/L/templates/T --text 'model output'
model_cmd | gwsr modelarmor +sanitize-response --template ...
```

## Tips

- Use for outbound safety (model -> user).
- For inbound safety (user -> model), use +sanitize-prompt.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-modelarmor](../gwsr-modelarmor/SKILL.md) — All filter user-generated content for safety commands
