---
name: gwsr-admin-user-suspend
description: "Google Workspace Admin SDK: Suspend (or with --unsuspend, restore) a user."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr admin +user-suspend --help"
---

# admin +user-suspend

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Suspend (or with --unsuspend, restore) a user

## Usage

```bash
gwsr admin +user-suspend --user <EMAIL>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--user` | ✓ | — | User email or ID |
| `--unsuspend` | — | — | Restore a suspended user instead |
| `--reason` | — | — | Suspension reason recorded on the account |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr admin +user-suspend --user ann@example.com --yes
gwsr admin +user-suspend --user ann@example.com --unsuspend
```

## Tips

- Suspending requires --yes (or a prompt on a terminal).

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-admin](../gwsr-admin/SKILL.md) — All manage users, groups, org units, devices, and roles (admin sdk directory) commands
