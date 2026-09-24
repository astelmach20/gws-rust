---
name: gwsr-events-subscribe
description: "Google Workspace Events: Subscribe to Workspace events and stream them as NDJSON."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr events +subscribe --help"
---

# events +subscribe

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

Subscribe to Workspace events and stream them as NDJSON

## Usage

```bash
gwsr events +subscribe
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--target` | — | — | Workspace resource URI (e.g., //chat.googleapis.com/spaces/SPACE_ID) |
| `--event-types` | — | — | Comma-separated CloudEvents types to subscribe to |
| `--project` | — | — | GCP project ID for Pub/Sub resources |
| `--subscription` | — | — | Existing Pub/Sub subscription name (skip setup) |
| `--max-messages` | — | 10 | Max messages per pull batch (default: 10) |
| `--poll-interval` | — | 5 | Seconds between pulls (default: 5) |
| `--once` | — | — | Pull once and exit |
| `--cleanup` | — | — | Delete created Pub/Sub resources on exit |
| `--no-ack` | — | — | Don't auto-acknowledge messages |
| `--output-dir` | — | — | Write each event to a separate JSON file in this directory |

## Examples

```bash
gwsr events +subscribe --target '//chat.googleapis.com/spaces/SPACE' --event-types 'google.workspace.chat.message.v1.created' --project my-project
gwsr events +subscribe --subscription projects/p/subscriptions/my-sub --once
gwsr events +subscribe ... --cleanup --output-dir ./events
```

## Tips

- Without --cleanup, Pub/Sub resources persist for reconnection.
- Press Ctrl-C to stop gracefully.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-events](../gwsr-events/SKILL.md) — All subscribe to google workspace events commands
