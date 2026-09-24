# Google Workspace CLI (`gwsr`) Context

The `gwsr` CLI provides dynamic access to Google Workspace APIs (Drive, Gmail, Calendar, Sheets, Admin, etc.) by parsing Discovery Documents at runtime.

## Rules of Engagement for Agents

* **Schema Discovery:** *If you don't know the exact JSON payload structure, run `gwsr schema <resource>.<method>` first to inspect the schema before executing.*
* **Context Window Protection:** *Workspace APIs (like Drive and Gmail) return massive JSON blobs. ALWAYS use field masks when listing or getting resources by appending `--params '{"fields": "id,name"}'` to avoid overwhelming your context window.*
* **Dry-Run Safety:** *Always use the `--dry-run` flag for mutating operations (create, update, delete) to validate your JSON payload before actual execution.*

## Core Syntax

```bash
gwsr <service> <resource> [sub-resource] <method> [flags]
```

Use `--help` to get help on the available commands.

```bash
gwsr --help
gwsr <service> --help
gwsr <service> <resource> --help
gwsr <service> <resource> <method> --help
```

### Key Flags

-   `--params '<JSON>'`: URL/query parameters (e.g., `id`, `q`, `pageSize`).
-   `--json '<JSON>'`: Request body for POST/PUT/PATCH methods.
-   `--page-all`: Auto-paginates results and outputs NDJSON (one JSON object per line).
-   `--fields '<MASK>'`: Limits the response fields (critical for AI context window efficiency).
-   `--upload <PATH>`: Files for multipart uploads (e.g., `drive files create`).
-   `--output <PATH>`: Destination for binary downloads (e.g., `drive files get`).
-   `--sanitize <TEMPLATE>`: Sanitizes output using Google Cloud Model Armor.

## Usage Patterns

### 1. Reading Data (GET/LIST)
Always use `--fields` to minimize tokens.

```bash
# List Drive files (efficient)
gwsr drive files list --params '{"q": "name contains \"Report\"", "pageSize": 10}' --fields "files(id,name,mimeType)"

# Get Gmail message details
gwsr gmail users messages get --params '{"userId": "me", "id": "MSG_123"}'
```

### 2. Writing Data (POST/PUT/PATCH)
Use `--json` for the request body.

```bash
# Send Email
gwsr gmail users messages send --params '{"userId": "me"}' --json '{"raw": "BASE64..."}'

# Create Spreadsheet
gwsr sheets spreadsheets create --json '{"properties": {"title": "Q4 Budget"}}'
```

### 3. Pagination (NDJSON)
Use `--page-all` for listing large collections. The output is Newline Delimited JSON.

```bash
# Stream all users
gwsr admin users list --params '{"domain": "example.com"}' --page-all
```

### 4. Schema Introspection
If unsure about parameters or body structure, check the schema:

```bash
gwsr schema drive.files.list
gwsr schema sheets.spreadsheets.create
```
