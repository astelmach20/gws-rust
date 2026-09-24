---
name: gwsr-drive-share
description: "Google Drive: Grant access to a file or folder."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
    cliHelp: "gwsr drive +share --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# drive +share

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules.

Grant access to a file or folder

## Usage

```bash
gwsr drive +share --file-id <ID> --role <ROLE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID |
| `--role` | ✓ | — | Role to grant (required; there is no default) |
| `--email` | — | — | Grant to this user (or group, with --group) |
| `--group` | — | — | Treat --email as a Google Group address |
| `--domain` | — | — | Grant to everyone in this domain |
| `--anyone` | — | — | Grant to anyone with the link |
| `--no-notify` | — | — | Do not send a notification email |
| `--message` | — | — | Custom message for the notification email |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr drive +share --file-id FILE_ID --email bob@example.com --role reader
gwsr drive +share --file-id FILE_ID --email team@example.com --group --role writer
gwsr drive +share --file-id FILE_ID --domain example.com --role commenter
gwsr drive +share --file-id FILE_ID --email bob@example.com --role owner --yes
```

## Tips

- --role is mandatory so access is never granted implicitly.
- Transferring ownership, or granting writer/organizer to a domain or to
- anyone with the link, always requires confirmation (--yes, or a prompt on
- a terminal). Other grants require it when GWSR_REQUIRE_CONFIRM=1.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-drive](../gwsr-drive/SKILL.md) — All manage files, folders, and shared drives commands
