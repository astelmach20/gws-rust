---
name: gwsr-events-subscribe
description: "Google Workspace Events: Subscribe to Workspace events and stream them as NDJSON."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr events +subscribe --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# events +subscribe

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

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
| `--project` | — | — | GCP project ID for Pub/Sub resources (or set GWSR_PROJECT_ID) |
| `--subscription` | — | — | Existing Pub/Sub subscription name (skip setup) |
| `--max-messages` | — | 10 | Maximum Pub/Sub messages per pull |
| `--poll-interval` | — | 5 | Seconds between pulls |
| `--max-failures` | — | 10 | Consecutive transient failures (429/5xx/network) tolerated before exiting |
| `--once` | — | — | Pull once and exit |
| `--cleanup` | — | — | Delete created Pub/Sub resources on exit |
| `--no-ack` | — | — | Do not acknowledge messages (they will be redelivered) |
| `--output-dir` | — | — | Write each event to a separate JSON file in this directory |

## Examples

```bash
gwsr events +subscribe --target '//chat.googleapis.com/spaces/SPACE' --event-types 'google.workspace.chat.message.v1.created' --project my-project
gwsr events +subscribe --subscription projects/p/subscriptions/my-sub --once
gwsr events +subscribe --subscription projects/p/subscriptions/my-sub --cleanup --output-dir ./events
```

## Tips

- stdout carries one JSON event per line. Transient errors are retried with
- backoff and reported as single-line JSON on stderr; the stream exits after
- --max-failures consecutive failures. Delivery is at-least-once.
- Without --cleanup, Pub/Sub resources persist for reconnection. Press Ctrl-C to stop.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-events](../gwsr-events/SKILL.md) — All subscribe to google workspace events commands
