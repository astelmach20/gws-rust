---
name: gwsr-admin-group-add-member
description: "Google Workspace Admin SDK: Add a user or group to a group."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr admin +group-add-member --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# admin +group-add-member

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Add a user or group to a group

## Usage

```bash
gwsr admin +group-add-member --group <EMAIL> --member <EMAIL>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--group` | ✓ | — | Group email or ID |
| `--member` | ✓ | — | Member email |
| `--role` | — | MEMBER | Membership role |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr admin +group-add-member --group eng@example.com --member ann@example.com
gwsr admin +group-add-member --group eng@example.com --member lead@example.com --role MANAGER
```

## Tips

- Adding someone who is already a member fails with the API's 409 error.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-admin](../gwsr-admin/SKILL.md) — All manage users, groups, org units, devices, and roles (admin sdk directory) commands
