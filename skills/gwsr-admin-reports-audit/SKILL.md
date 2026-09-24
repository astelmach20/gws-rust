---
name: gwsr-admin-reports-audit
description: "Google Workspace Admin SDK: Query audit activity (Reports API)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr admin-reports +audit --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# admin-reports +audit

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Query audit activity (Reports API)

## Usage

```bash
gwsr admin-reports +audit --application <APP>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--application` | ✓ | — | Application whose audit log to read |
| `--user` | — | — | Only this user's activity (email or ID; default: all users) |
| `--event` | — | — | Only this event name, e.g. login_failure |
| `--since` | — | 1d | Start: RFC 3339 time, or a look-back like 24h / 7d (default: 1d) |
| `--until` | — | — | End: RFC 3339 time (default: now) |
| `--filter` | — | — | Event parameter filter, e.g. 'doc_id==XYZ' |
| `--limit` | — | 1000 | Maximum activities (default: 1000) |

## Examples

```bash
gwsr admin-reports +audit --application login --event login_failure --since 7d
gwsr admin-reports +audit --application drive --user ann@example.com --since 2026-06-01T00:00:00Z
gwsr admin-reports +audit --application admin --format table
```

## Tips

- Read-only. Newest first. The output says when --limit cut results short.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-admin-reports](../gwsr-admin-reports/SKILL.md) — All audit logs and usage reports commands
