---
name: gwsr-events-renew
description: "Google Workspace Events: Renew or reactivate Workspace Events subscriptions."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr events +renew --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# events +renew

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Renew or reactivate Workspace Events subscriptions

## Usage

```bash
gwsr events +renew
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--subscription-id` | — | — | Subscription to renew (subscriptions/SUB_ID or SUB_ID) |
| `--all` | — | — | Renew every subscription for --event-types that expires within --within |
| `--event-types` | — | — | Comma-separated event types (required with --all; selects the OAuth scope) |
| `--within` | — | 1h | Time window for --all (e.g., 30m, 1h, 2d) |
| `--reactivate` | — | — | Reactivate a SUSPENDED subscription instead of extending its expiry |

## Examples

```bash
gwsr events +renew --subscription-id subscriptions/SUB_ID
gwsr events +renew --subscription-id SUB_ID --reactivate
gwsr events +renew --all --event-types google.workspace.chat.message.v1.created --within 2d
```

## Tips

- Renewing sets the subscription TTL to the maximum allowed.
- Use --all from a cron job to keep subscriptions alive.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-events](../gwsr-events/SKILL.md) — All subscribe to google workspace events commands
