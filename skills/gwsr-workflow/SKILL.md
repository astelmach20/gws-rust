---
name: gwsr-workflow
description: "Google Workflow: Cross-service productivity workflows."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr workflow --help"
---

# workflow (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gwsr generate-skills` to create it.

```bash
gwsr workflow <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+standup-report`](../gwsr-workflow-standup-report/SKILL.md) | Today's meetings + open tasks as a standup summary |
| [`+meeting-prep`](../gwsr-workflow-meeting-prep/SKILL.md) | Prepare for your next meeting: agenda, attendees, and linked docs |
| [`+email-to-task`](../gwsr-workflow-email-to-task/SKILL.md) | Convert a Gmail message into a Google Tasks entry |
| [`+weekly-digest`](../gwsr-workflow-weekly-digest/SKILL.md) | Weekly summary: this week's meetings + unread email count |
| [`+file-announce`](../gwsr-workflow-file-announce/SKILL.md) | Announce a Drive file in a Chat space |

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr workflow --help

# Inspect a method's required params, types, and defaults
gwsr schema workflow.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

