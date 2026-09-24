# gwsr: Google Workspace CLI context for agents

`gwsr` gives dynamic access to every Google Workspace API (Drive, Gmail, Calendar, Sheets, Docs, Admin and more). It builds its commands from Google Discovery documents at runtime, and has hand-written `+helper` commands for multi-step jobs.

## Rules of engagement

- **Inspect before you build.** Run `gwsr schema <service>.<resource>.<method>` to see parameters and the request body schema. Run `gwsr commands <service>` for a JSON inventory of every method and helper with its flags.
- **Protect your context window.** Responses can be huge. Use `--fields` for a partial response (`--fields 'files(id,name)'`), `--jq` to extract what you need, and `--page-items` / `--page-limit` when listing.
- **Dry-run mutations.** `--dry-run` validates the request and prints it as JSON without sending it or loading credentials.
- **Never pass `--yes` on your own authority.** Destructive actions (deletes, clears, empty trash, permanent deletes, destructive helpers) exit with code `7` and send nothing unless `--yes` is given. When that happens, ask the user, then re-run with `--yes`.
- **Branch on exit codes, not on text.**

## Output and errors

- stdout carries only the result: JSON by default (NDJSON when paginating or streaming). Logs and warnings go to stderr.
- On failure stdout is empty, and stderr has one JSON object: `{"error":{"code":…,"message":…,"reason":…,"retryable":…,"hint":…}}`. Follow `hint` when it is present; it often contains the exact command to run (for example a `gwsr auth login --scopes …` for a missing scope).

| Exit | Meaning | What to do |
|---|---|---|
| 0 | Success | |
| 1 | API error | Fix the request; don't retry unchanged |
| 2 | Auth error | Ask the user to run `gwsr auth login` (or the command in `hint`) |
| 3 | Validation error | Fix the arguments; see `gwsr schema …` |
| 4 | Discovery error | Check the service name or network |
| 5 | Internal error | Report it |
| 6 | Retryable API error (429/5xx/rate limit) | Retry with backoff |
| 7 | Confirmation required | Ask the user, then re-run with `--yes` |
| 8 | Configuration error | Ask the user to fix the setting the message names. Every `GWSR_*` variable is checked at startup, so a bad or unknown one (e.g. a typo like `GWSR_TIMEOUT_SECS`) fails every command until it is fixed or unset |
| 9 | Credential store error (keyring, stored credentials) | Ask the user; `gwsr auth status` shows the stored credential state |
| 10 | Network error (no response) | For creates/sends, check whether it was applied before retrying |
| 11 | Output blocked by Model Armor | Don't retry to get around it; the content was withheld on purpose |

## Syntax

```bash
gwsr <service> <resource> [sub-resource] <method> [--params JSON] [--json JSON] [flags]
gwsr <service> +<helper> [flags]
gwsr <api>:<version> <resource> <method> ...     # any Discovery API, e.g. youtube:v3
gwsr <service> [<resource> [<method>]] --help
```

### Key flags

| Flag | Purpose |
|---|---|
| `--params JSON` | URL path and query parameters (`@file.json` or `-` for stdin also work) |
| `--json JSON` | Request body for POST/PUT/PATCH (`@file`/`-` too) |
| `--fields MASK` | Partial response |
| `--page-all` / `--page-items` | Fetch every page as NDJSON: one line per page / per item |
| `--page-limit N` | Stop after N pages (default unlimited) |
| `--format json\|table\|yaml\|csv`, `--jq EXPR`, `--columns a,b` | Output shaping |
| `--upload PATH` | Media upload (e.g. `drive files create`) |
| `-o PATH` / `-o -` | Write a binary response to a file / raw to stdout |
| `--wait` | Poll a long-running operation until it finishes |
| `--dry-run` | Print the request instead of sending it |
| `-y, --yes` | Confirm a destructive action (only after the user agrees) |
| `--sanitize TEMPLATE` | Screen the response with Model Armor |
| `--profile NAME` | Use another credential profile |

Unknown `--params` or `--json` fields are rejected before sending, with a "did you mean" suggestion.

## Examples

```bash
# Read (always narrow the fields)
gwsr drive files list --params '{"q": "name contains \"Report\"", "pageSize": 10}' --fields 'files(id,name,mimeType)'
gwsr gmail users messages get --params '{"userId": "me", "id": "MSG_ID", "format": "metadata"}'
gwsr gmail +search --query 'from:alice newer_than:7d' --max 10
gwsr calendar +agenda --today

# Write
gwsr sheets spreadsheets create --json '{"properties": {"title": "Q4 Budget"}}'
gwsr sheets +append --spreadsheet-id SHEET_ID --range 'Sheet1!A1' --values 'Alice,95'
gwsr gmail +send --to alice@example.com --subject 'Hi' --body 'Hello' --draft

# Paginate
gwsr admin users list --params '{"customer": "my_customer"}' --page-items=users --jq '.primaryEmail'

# Download
gwsr drive files get --params '{"fileId": "FILE_ID", "alt": "media"}' -o report.pdf

# Destructive: exits 7 without --yes
gwsr drive files delete --params '{"fileId": "FILE_ID"}' --yes
```

Shell tip: Sheets ranges contain `!`. Wrap them in single quotes: `--range 'Sheet1!A1:C10'`.

The `skills/` directory has a skill per service and per helper, plus the shared rules in `skills/gwsr-shared/SKILL.md`.
