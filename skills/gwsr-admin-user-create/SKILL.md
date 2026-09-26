---
name: gwsr-admin-user-create
description: "Google Workspace Admin SDK: Create a user account."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr admin +user-create --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# admin +user-create

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Create a user account

## Usage

```bash
gwsr admin +user-create --email <EMAIL> --given-name <NAME> --family-name <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--email` | ✓ | — | Primary email |
| `--given-name` | ✓ | — | First name |
| `--family-name` | ✓ | — | Last name |
| `--org-unit` | — | — | Organizational unit path (default: /) |
| `--password-file` | — | — | Read the initial password from a file, or '-' for stdin (default: generate one) |
| `--no-password-change` | — | — | Do not force a password change at first sign-in |

## Examples

```bash
gwsr admin +user-create --email ann@example.com --given-name Ann --family-name Lee
printf '%s' "$PW" | gwsr admin +user-create --email ann@example.com --given-name Ann --family-name Lee --password-file -
```

## Tips

- Without --password-file a random 24-character password is generated and
- printed once as initialPassword; share it over a secure channel.
- The user must change the password at first sign-in unless --no-password-change.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-admin](../gwsr-admin/SKILL.md) — All manage users, groups, org units, devices, and roles (admin sdk directory) commands
