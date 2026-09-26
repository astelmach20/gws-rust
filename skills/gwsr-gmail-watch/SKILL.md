---
name: gwsr-gmail-watch
description: "Gmail: Watch for new emails and stream them as NDJSON."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +watch --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +watch

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Watch for new emails and stream them as NDJSON

## Usage

```bash
gwsr gmail +watch
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--project` | — | — | GCP project ID for Pub/Sub resources (or set GWSR_PROJECT_ID) |
| `--subscription` | — | — | Existing Pub/Sub subscription name (skip setup) |
| `--topic` | — | — | Existing Pub/Sub topic with Gmail publish permission already granted |
| `--label-ids` | — | — | Comma-separated Gmail label IDs to watch (e.g., INBOX,UNREAD) |
| `--max-messages` | — | 10 | Maximum Pub/Sub messages per pull |
| `--poll-interval` | — | 5 | Seconds between pulls |
| `--max-failures` | — | 10 | Consecutive transient failures (429/5xx/network) tolerated before exiting |
| `--msg-format` | — | full | Gmail message format |
| `--once` | — | — | Pull once and exit |
| `--cleanup` | — | — | Delete created Pub/Sub resources on exit |
| `--output-dir` | — | — | Write each message to a separate JSON file in this directory |

## Examples

```bash
gwsr gmail +watch --project my-gcp-project
gwsr gmail +watch --project my-project --label-ids INBOX --once
gwsr gmail +watch --subscription projects/p/subscriptions/my-sub
gwsr gmail +watch --project my-project --cleanup --output-dir ./emails
```

## Tips

- stdout carries one JSON message per line. Transient errors are retried with
- backoff and reported as single-line JSON on stderr; the watcher exits after
- --max-failures consecutive failures. Delivery is at-least-once.
- Gmail watch expires after 7 days; re-run to renew. Press Ctrl-C to stop.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
