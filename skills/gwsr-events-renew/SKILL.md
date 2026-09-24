---
name: gwsr-events-renew
description: "Google Workspace Events: Renew/reactivate Workspace Events subscriptions."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr events +renew --help"
---

# events +renew

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Renew/reactivate Workspace Events subscriptions

## Usage

```bash
gwsr events +renew
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--name` | — | — | Subscription name to reactivate (e.g., subscriptions/SUB_ID) |
| `--all` | — | — | Renew all subscriptions expiring within --within window |
| `--within` | — | 1h | Time window for --all (e.g., 1h, 30m, 2d) |

## Examples

```bash
gwsr events +renew --name subscriptions/SUB_ID
gwsr events +renew --all --within 2d
```

## Tips

- Subscriptions expire if not renewed periodically.
- Use --all with a cron job to keep subscriptions alive.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-events](../gwsr-events/SKILL.md) — All subscribe to google workspace events commands
