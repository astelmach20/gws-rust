---
name: gwsr-shared
description: "gwsr CLI: Shared patterns for authentication, global flags, and output formatting."
metadata:
  version: 0.23.0
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gwsr
---
<!-- gwsr generated skill: do not edit by hand -->

# gwsr — Shared Reference

## Installation

The `gwsr` binary must be on `$PATH`. See the project README for install options.

## Authentication

```bash
# Browser-based OAuth (read-only scopes by default; add --write or --scopes for more)
gwsr auth login
gwsr auth login --write -s drive,gmail     # read-write for selected services
gwsr auth login --no-localhost             # headless/SSH: paste the redirected URL back

# Service account or authorized-user JSON (no login needed)
export GWSR_CREDENTIALS_FILE=/path/to/key.json

# Check what is active and which scopes were granted
gwsr auth status
```

Use `--profile NAME` (or `GWSR_PROFILE`) to pick a credential profile, and
`--impersonate USER` (service accounts with domain-wide delegation) to act as a user.

## Global Flags

| Flag | Description |
|------|-------------|
| `--format <FORMAT>` | Output format: `json` (default), `table`, `yaml`, `csv` |
| `--jq <EXPR>` | Filter the output with a jq expression; string results print raw |
| `--columns <A,B>` | Columns to show for `table` / `csv` output (dot paths for nested fields) |
| `--compact` / `--pretty` | Force compact or pretty JSON (default: compact when piped, pretty on a terminal) |
| `--dry-run` | Validate locally and print the request without sending it |
| `--sanitize <TEMPLATE>` | Screen responses through a Model Armor template |
| `--api-version <V>` | Override the API version (also `<service>:<version>`, e.g. `youtube:v3`) |
| `--profile <NAME>` | Credential profile |
| `--impersonate <EMAIL>` | Service accounts only: act as this user (domain-wide delegation) |
| `-v` / `-q` | More / less diagnostic logging on stderr |

Run `gwsr commands` for a machine-readable (JSON) inventory of every service, resource,
method and helper with its flags.

## Output, Errors and Exit Codes

stdout carries only results: JSON by default (NDJSON for `--page-all`, `--page-items`
and streaming helpers). Logs and progress go to stderr. On failure stdout is empty and
stderr holds one JSON object: `{"error":{"code","message","reason","retryable","hint"?}}`
(human-readable text with a non-JSON `--format`).

Exit codes: `0` success, `1` API error (permanent), `2` auth, `3` validation,
`4` discovery, `5` internal, `6` API error that is safe to retry (429 / 5xx / rate limit),
`7` confirmation required (nothing was sent; re-run with `--yes` once the user agrees),
`8` configuration (`configError`), `9` credential store (`credentialStoreError`: keyring or
stored credentials), `10` network (`networkError`: no response; a non-idempotent call may
have been applied, so check before retrying), `11` output blocked by Model Armor
(`sanitizationBlocked`: `--sanitize` in block mode; the request itself succeeded).

## CLI Syntax

```bash
gwsr <service> <resource> [sub-resource] <method> [flags]
```

### Method Flags

| Flag | Description |
|------|-------------|
| `--params '{"key": "val"}'` | URL/query parameters (`@file` or `-` for stdin also work) |
| `--json '{"key": "val"}'` | Request body (`@file` or `-` for stdin also work) |
| `--fields <MASK>` | Partial response, e.g. `'files(id,name)'` — keeps output small |
| `-o, --output <PATH>` | Write the response payload to a file (required for binary media) |
| `--upload <PATH>` | Upload file content (resumable above 5 MiB) |
| `--page-all` | Fetch every page, one JSON line per page (NDJSON) |
| `--page-items[=FIELD]` | Fetch every page, one JSON line per item |
| `--page-limit <N>` | Stop after N pages (default: unlimited) |
| `--wait` | Poll a returned long-running operation until it finishes |
| `-y, --yes` | Confirm a destructive method (required when not on a terminal) |

Unknown `--params` names and body fields are rejected; run `gwsr schema <service>.<resource>.<method>`
to see what a method accepts.

## Security Rules

- **Never** output secrets (API keys, tokens) directly
- **Always** confirm with the user before executing write/delete commands; only then pass `--yes`
- Prefer `--dry-run` to preview mutating requests
- Use `--sanitize` for PII/content safety screening

## Shell Tips

- **Quote arguments containing `!` with single quotes.** Sheet ranges like `Sheet1!A1` contain `!`, which interactive bash (and zsh with `BANG_HIST`) history-expands inside double quotes. Single quotes are never expanded:
  ```bash
  # CORRECT: single quotes are safe in bash and zsh
  gwsr sheets +read --spreadsheet-id ID --range 'Sheet1!A1:D10'

  # WRONG: interactive bash expands `!A1` inside double quotes
  gwsr sheets +read --spreadsheet-id ID --range "Sheet1!A1:D10"
  ```
- **JSON with double quotes:** Wrap `--params` and `--json` values in single quotes so the shell does not interpret the inner double quotes:
  ```bash
  gwsr drive files list --params '{"pageSize": 5}'
  ```
