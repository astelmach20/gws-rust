---
name: gwsr-meet-create
description: "Google Meet: Create a Meet meeting space and print its link."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr meet +create --help"
---

# meet +create

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Create a Meet meeting space and print its link

## Usage

```bash
gwsr meet +create
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--access-type` | — | — | Who can join without knocking (default: your organization's setting) |

## Examples

```bash
gwsr meet +create
gwsr meet +create --access-type restricted
```

## Tips

- Prints meetingUri and meetingCode. To attach a Meet link to a calendar
- event use `gwsr calendar +insert --meet` instead.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-meet](../gwsr-meet/SKILL.md) — All manage google meet conferences commands
