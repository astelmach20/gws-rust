---
name: gwsr-docs
description: "Read and write Google Docs."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr docs --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# docs (v1)

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

```bash
gwsr docs <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+create`](../gwsr-docs-create/SKILL.md) | Create a new document, optionally with content |
| [`+read`](../gwsr-docs-read/SKILL.md) | Read a document as plain text or Markdown |
| [`+write`](../gwsr-docs-write/SKILL.md) | Append text or Markdown to the end of a document |
| [`+replace`](../gwsr-docs-replace/SKILL.md) | Find and replace text throughout a document |

## API Resources

### documents

  - `batchUpdate` — Applies one or more updates to the document. Each request is validated before being applied. If any request is not valid, then the entire request will fail and nothing will be applied. Some requests have replies to give you some information about how they are applied. Other requests do not need to return information; these each return an empty reply. The order of replies matches that of the requests.
  - `create` — Creates a blank document using the title given in the request. Other fields in the request, including any provided content, are ignored. Returns the created document.
  - `get` — Gets the latest version of the specified document.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gwsr docs --help

# Inspect a method's required params, types, and defaults
gwsr schema docs.<resource>.<method>
```

Use `gwsr schema` output to build your `--params` and `--json` flags.

