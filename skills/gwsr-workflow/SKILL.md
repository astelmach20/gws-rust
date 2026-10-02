---
name: gwsr-workflow
description: "Google Workflow: Cross-service productivity workflows."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr workflow --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# workflow (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr workflow <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+standup-report`](../gwsr-workflow-standup-report/SKILL.md) | Today's meetings and open tasks as a standup summary |
| [`+meeting-prep`](../gwsr-workflow-meeting-prep/SKILL.md) | Prepare for your next meeting: agenda, attendees, and links |
| [`+email-to-task`](../gwsr-workflow-email-to-task/SKILL.md) | Convert a Gmail message into a Google Tasks entry |
| [`+weekly-digest`](../gwsr-workflow-weekly-digest/SKILL.md) | Weekly summary: the next 7 days of meetings and your unread email count |
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

