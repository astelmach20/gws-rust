---
name: gwsr-gmail-filter
description: "Gmail: List, create, or delete Gmail filters."
metadata:
  version: 0.23.0
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
gwsr gmail +filter create
```

#### Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |
| `--from` | — | — | Match sender |
| `--to` | — | — | Match recipient |
| `--subject` | — | — | Match subject |
| `--query` | — | — | Match a Gmail search query |
| `--exclude` | — | — | Exclude messages matching this query |
| `--has-attachment` | — | — | Match only messages with attachments |
| `--add-label` | — | — | Label names or IDs to apply (repeatable or comma-separated) |
| `--remove-label` | — | — | Label names or IDs to remove (repeatable or comma-separated) |
| `--archive` | — | — | Skip the Inbox |
| `--mark-read` | — | — | Mark as read |
| `--star` | — | — | Star the message |
| `--important` | — | — | Mark as important |
| `--never-important` | — | — | Never mark as important |
| `--trash` | — | — | Move to Trash |
| `--never-spam` | — | — | Never send to Spam |
| `--forward` | — | — | Forward to this verified forwarding address (needs --yes when GWSR_REQUIRE_CONFIRM=1) |

### delete

Delete a filter

```bash
gwsr gmail +filter delete --filter-id <ID>
```

#### Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--filter-id` | ✓ | — | ID of the filter to delete |
| `--yes` | — | — | Confirm this action without prompting (required when not on a terminal) |

## Examples

```bash
gwsr gmail +filter list
gwsr gmail +filter create --from newsletter@example.com --add-label Newsletters --archive
gwsr gmail +filter create --query 'subject:(invoice OR receipt)' --add-label Receipts
gwsr gmail +filter delete --filter-id ANe1Bmj... --yes
```

## Tips

- Requires the gmail.settings.basic scope (gwsr auth login --scopes gmail.settings.basic).
- Label names must already exist. Deleting a filter always requires --yes;
- a filter with --forward requires it when GWSR_REQUIRE_CONFIRM=1.
- With --sanitize, create screens the new filter before creating it (block mode:
- a match or a Model Armor failure creates nothing); list screens its output.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gwsr-shared](../gwsr-shared/SKILL.md) — Global flags and auth
- [gwsr-gmail](../gwsr-gmail/SKILL.md) — All send, read, and manage email commands
