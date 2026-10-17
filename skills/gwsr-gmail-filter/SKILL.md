---
name: gwsr-gmail-filter
description: "Gmail: List, create, or delete Gmail filters."
metadata:
  version: 0.24.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-shared
    cliHelp: "gwsr gmail +filter --help"
---
<!-- gwsr generated skill: do not edit by hand -->

# gmail +filter

> **PREREQUISITE:** Read `../gwsr-shared/SKILL.md` for auth, global flags, and security rules. If it is missing, install it with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-shared`.

List, create, or delete Gmail filters

## Usage

```bash
gwsr gmail +filter <command>
```

### list

List filters

```bash
gwsr gmail +filter list
```

### create

Create a filter

```bash
gwsr gmail +filter create --from <TEXT> --add-label <LABEL>
```

Requires at least one of: `--from <TEXT>`, `--to <TEXT>`, `--subject <TEXT>`, `--query <QUERY>`, `--exclude <QUERY>`, `--has-attachment`.

Requires at least one of: `--add-label <LABEL>`, `--remove-label <LABEL>`, `--archive`, `--mark-read`, `--star`, `--important`, `--never-important`, `--trash`, `--never-spam`, `--forward <EMAIL>`.

#### Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--yes` | — | — | Confirm without prompting. When stdin or stderr is not a terminal, destructive actions require it; outbound ones (send, share, notify, run code) only when GWSR_REQUIRE_CONFIRM=1 |
| `--from` | at least one | — | Match sender |
| `--to` | at least one | — | Match recipient |
| `--subject` | at least one | — | Match subject |
| `--query` | at least one | — | Match a Gmail search query |
| `--exclude` | at least one | — | Exclude messages matching this query |
| `--has-attachment` | at least one | — | Match only messages with attachments |
| `--add-label` | at least one | — | Label names or IDs to apply (repeatable or comma-separated) |
| `--remove-label` | at least one | — | Label names or IDs to remove (repeatable or comma-separated) |
| `--archive` | at least one | — | Skip the Inbox |
| `--mark-read` | at least one | — | Mark as read |
| `--star` | at least one | — | Star the message |
| `--important` | at least one | — | Mark as important |
| `--never-important` | at least one | — | Never mark as important |
| `--trash` | at least one | — | Move to Trash |
| `--never-spam` | at least one | — | Never send to Spam |
| `--forward` | at least one | — | Forward to this verified forwarding address (needs --yes when GWSR_REQUIRE_CONFIRM=1) |

### delete

Delete a filter

```bash
gwsr gmail +filter delete --filter-id <ID>
```

#### Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--filter-id` | ✓ | — | ID of the filter to delete |
| `--yes` | — | — | Confirm without prompting. When stdin or stderr is not a terminal, destructive actions require it; outbound ones (send, share, notify, run code) only when GWSR_REQUIRE_CONFIRM=1 |

## Examples

```bash
gwsr gmail +filter list
gwsr gmail +filter create --from newsletter@example.com --add-label Newsletters --archive
gwsr gmail +filter create --query 'subject:(invoice OR receipt)' --add-label Receipts
gwsr gmail +filter delete --filter-id ANe1Bmj... --yes
```

## Tips

- Requires the gmail.settings.basic scope (gwsr auth login --scopes gmail.settings.basic).
- Label names must already exist, and looking them up needs gmail.readonly (or gmail.modify) as well. Deleting a filter always requires --yes; a filter with --forward requires it when GWSR_REQUIRE_CONFIRM=1.
- With --sanitize, create screens the new filter before creating it (block mode: a match or a Model Armor failure creates nothing); list screens its output.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
