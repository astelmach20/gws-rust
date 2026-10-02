---
name: gwsr-script
description: "Manage Google Apps Script projects."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr script --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# script (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

```bash
gwsr script <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+push`](../gwsr-script-push/SKILL.md) | Replace a project's files with local files |
| [`+pull`](../gwsr-script-pull/SKILL.md) | Download a project's files into a local directory |
| [`+run`](../gwsr-script-run/SKILL.md) | Run a function in a deployed Apps Script project |
| [`+logs`](../gwsr-script-logs/SKILL.md) | List recent executions of a project |

## API Resources

### processes

  - `list` — List information about processes made by or on behalf of a user, such as process type and current status.
  - `listScriptProcesses` — List information about a script's executed processes, such as process type and current status.

### projects

  - `create` — Creates a new, empty script project with no script files and a base manifest file.
  - `get` — Gets a script project's metadata.
  - `getContent` — Gets the content of the script project, including the code source and metadata for each script file.
  - `getMetrics` — Get metrics data for scripts, such as number of executions and active users.
  - `updateContent` — Updates the content of the specified script project. This content is stored as the HEAD version, and is used when the script is executed as a trigger, in the script editor, in add-on preview mode, or as a web app or Apps Script API in development mode. This clears all the existing files in the project.
  - `deployments` — Operations on the 'deployments' resource
  - `versions` — Operations on the 'versions' resource

### scripts

  - `run` — 

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr script --help

# Inspect a method's required params, types, and defaults
gwsr schema script.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

