---
name: gwsr-chat-send
description: "Google Chat: Send a message to a space."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr chat +send --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# chat +send

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

Send a message to a space

## Usage

```bash
gwsr chat +send --space-id <ID> --text <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--space-id` | ✓ | — | Space ID, as 'AAAA...' or 'spaces/AAAA...' |
| `--text` | ✓ | — | Message text (Chat formatting such as *bold* is supported) |
| `--thread` | — | — | Reply in this thread (spaces/SPACE/threads/THREAD); falls back to a new thread if it no longer exists |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr chat +send --space-id spaces/AAAAxxxx --text 'Hello team!'
gwsr chat +send --space-id AAAAxxxx --thread spaces/AAAAxxxx/threads/TTTT --text 'Done.'
```

## Tips

- Use 'gwsr chat +spaces' to find space IDs.
- Requires confirmation when GWSR_REQUIRE_CONFIRM=1.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-chat](../gwsr-chat/SKILL.md) — All manage chat spaces and messages commands
