# Helper Commands (`+verb`) — Guidelines

## Design Principle

The core design of `gwsr` is **schema-driven**: commands are dynamically generated from Google Discovery Documents at runtime. This avoids maintaining a hardcoded, unbounded argument surface. **Helpers must complement this design, not duplicate it.**

## When a Helper is Justified

A `+helper` command should exist only when it provides value that Discovery-based commands **cannot**:

| Justification | Example | Why Discovery Can't Do It |
|---|---|---|
| **Multi-step orchestration** | `+subscribe` | Creates Pub/Sub topic → subscription → Workspace Events subscription (3 APIs) |
| **Format translation** | `+write` | Transforms Markdown → Docs `batchUpdate` JSON |
| **Multi-API composition** | `+triage` | Lists messages then fetches N metadata payloads concurrently |
| **Complex body construction** | `+send`, `+reply` | Builds RFC 2822 MIME from simple flags |
| **Multipart upload** | `+upload` | Handles resumable upload protocol with progress |
| **Workflow recipes** | `+standup-report` | Chains calls across multiple services |

**Litmus test:** Can the user achieve the same result with `gwsr <service> <resource> <method> --params '{...}'`? If yes, don't add a helper.

## Anti-Patterns

### Anti-pattern 1: Single API Call Wrapper

If a helper wraps one API call that Discovery already exposes, reject it.

**Real example:** `+revisions` (PR #563) wrapped `gwsr drive files-revisions list` — same single API call, zero added value.

### Anti-pattern 2: Unbounded Flag Accumulation

Adding flags to expose data that is already in the API response creates unbounded surface area.

**Real example:** `--thread-id`, `--delivered-to`, `--sent-last` on `+triage` (PR #597) — all three values are already present in the Gmail API response. Agents and users should extract them with `--jq` or `--format`, not new flags.

**Why this is harmful:** Every API response contains dozens of fields. If we add a flag for each one, helpers become unbounded maintenance burdens — the exact problem Discovery-driven design solves.

### Anti-pattern 3: Duplicating Discovery Parameters

Don't re-expose Discovery-defined parameters (e.g., `pageSize`, `fields`, `orderBy`) as custom helper flags. Use `--params` passthrough instead.

## Flag Design Rules

Helper flags must control **orchestration logic**, not API parameters or output fields.

### Good flags (control orchestration)

| Flag | Helper | Why It's Good |
|---|---|---|
| `--spreadsheet-id`, `--range` | `+read` | Identifies which resource to operate on |
| `--to`, `--subject`, `--body` | `+send` | Inputs to MIME construction (format translation) |
| `--subscription` | `+subscribe` | Switches between "create new" vs. "use existing" orchestration path |
| `--target`, `--project` | `+subscribe` | Required for multi-service resource creation |

### Bad flags (expose API response data)

| Flag | Why It's Bad | Alternative |
|---|---|---|
| `--thread-id` | Already in API response | `--jq '.threadId'` |
| `--delivered-to` | Already in response headers | `--jq` over `.payload.headers` |
| `--include-labels` | Output field filtering | `--jq` or `--columns` |

### Decision Checklist for New Flags

1. Does this flag control **what API call to make** or **how to orchestrate** multiple calls? Add it.
2. Does this flag control **what data appears in output**? Use `--jq` / `--format` instead.
3. Does this flag duplicate a Discovery parameter? Use `--params` instead.
4. Could the user achieve this with existing flags plus post-processing? Don't add it.

### Naming conventions

- Resource IDs are `--<noun>-id` (`--document-id`, `--spreadsheet-id`, `--message-id`, `--calendar-id`, `--space-id`).
- People and groups are `--email`, `--user`, `--group`, `--member`.
- A local output file is `--output PATH` (`-` for stdout), built with `http::OutputTarget`; it never overwrites without `--overwrite`.
- Result counts are `--limit N`.

## Architecture

Helpers implement the `Helper` trait in `mod.rs` and are registered in `get_helper()`.

- **`inject_commands`** adds the `+verb` subcommands to the service command. Helper commands are always shown, whatever the authentication state.
- **`handle`** runs the command. It returns `Ok(true)` if it handled the command, or `Ok(false)` to fall back to the generated Discovery commands.

## Adding a new helper: checklist

1. **Passes the litmus test:** it can't be done with a single Discovery command.
2. **Flags are bounded:** only flags that control orchestration, not API parameters or output fields.
3. **Uses the shared infrastructure:**
   - `http::Api` for every request. It loads credentials for the scopes you pass, records planned requests for `--dry-run`, handles pagination and applies `--sanitize`. Never build an authenticated `reqwest` request yourself.
   - `crate::validate::validate_resource_name()` for user-supplied resource IDs, and `encode_path_segment()` for URL path segments.
   - `crate::validate::validate_safe_file_path()` / `validate_safe_output_dir()` / `validate_safe_dir_path()` for local paths, and `http::OutputTarget` for `--output`.
   - `crate::confirm::confirm()` with `Impact::Destructive` or `Impact::Outbound` before an action that deletes data or reaches other people, and `confirm::with_yes()` on the command.
   - Results go to stdout as JSON through the shared output path; progress and warnings go to stderr.
4. **Supports `--dry-run`:** it prints the planned requests and needs no credentials.
5. **Has tests:** command registration, required arguments, the happy path (wiremock) and the rejection paths.
6. **Regenerate the skills** (`just skills`) and add a changeset.

### Development steps

1. Create `src/helpers/<service>.rs` (or a directory for larger helpers, like `gmail/`).
2. Implement the `Helper` trait.
3. Register it in `get_helper()` in `src/helpers/mod.rs`.
4. **Prefix** every command with `+` (for example `+create`).
