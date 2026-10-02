---
name: gwsr-workflow-file-announce
description: "Google Workflow: Announce a Drive file in a Chat space."
metadata:
  version: 0.23.1
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr workflow +file-announce --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# workflow +file-announce

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Announce a Drive file in a Chat space

## Usage

```bash
gwsr workflow +file-announce --file-id <ID> --space-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID to announce |
| `--space-id` | ✓ | — | Chat space (SPACE_ID or spaces/SPACE_ID) |
| `--message` | — | — | Custom announcement text (the file link is appended) |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr workflow +file-announce --file-id FILE_ID --space-id ABC123
gwsr workflow +file-announce --file-id FILE_ID --space-id spaces/ABC123 --message 'Check this out!'
```

## Tips

- Sends a Chat message. With GWSR_REQUIRE_CONFIRM=1 it requires --yes.
- Upload the file first with gwsr drive +upload, then announce it here.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-workflow](../gwsr-workflow/SKILL.md) — All cross-service productivity workflows commands
