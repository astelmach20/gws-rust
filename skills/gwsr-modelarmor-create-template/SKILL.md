---
name: gwsr-modelarmor-create-template
description: "Google Model Armor: Create a new Model Armor template."
metadata:
  version: 0.22.5
  openclaw:
    category: "security"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr modelarmor +create-template --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# modelarmor +create-template

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Create a new Model Armor template

## Usage

```bash
gwsr modelarmor +create-template --project <PROJECT> --location <LOCATION> --template-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--project` | ✓ | — | GCP project ID |
| `--location` | ✓ | — | GCP location (e.g. us-central1) |
| `--template-id` | ✓ | — | Template ID to create |
| `--preset` | — | — | Use a preset template: jailbreak |
| `--json` | — | — | JSON body for the template configuration (instead of --preset) |

## Examples

```bash
gwsr modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --preset jailbreak
gwsr modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --json '{...}'
```

## Tips

- Defaults to the built-in jailbreak preset if neither --preset nor --json is given.
- Use the resulting template name with +sanitize-prompt and +sanitize-response.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-modelarmor](../gwsr-modelarmor/SKILL.md) — All filter user-generated content for safety commands
