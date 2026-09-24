# AGENTS.md

## Project Overview

`gwsr` is a Rust CLI tool for interacting with Google Workspace APIs. It dynamically generates its command surface at runtime by parsing Google Discovery Service JSON documents.

> [!IMPORTANT]
> **Dynamic Discovery**: This project does NOT use generated Rust crates (e.g., `google-drive3`) for API interaction. Instead, it fetches the Discovery JSON at runtime and builds `clap` commands dynamically. When adding a new service, you only need to register it in `crates/gws-rust-core/src/services.rs` and verify the Discovery URL pattern in `crates/gws-rust-core/src/discovery.rs`. Do NOT add new crates to `Cargo.toml` for standard Google APIs.

> [!NOTE]
> **Package Manager**: Use `pnpm` instead of `npm` for Node.js package management in this repository.

## Build & Test

> [!IMPORTANT]
> **Test Coverage**: The `codecov/patch` check requires that new or modified lines are covered by tests. When adding code, extract testable helper functions rather than embedding logic in `main`/`run` where it's hard to unit-test. Run `cargo test` locally and verify new branches are exercised.

```bash
cargo build          # Build in dev mode
cargo clippy -- -D warnings  # Lint check
cargo test           # Run tests
```

## Changesets

Every PR must include a changeset file. Create one at `.changeset/<descriptive-name>.md`:

```markdown
---
"gws-rust": patch
---

Brief description of the change
```

Use `patch` for fixes/chores, `minor` for new features, `major` for breaking changes. The CI policy check will fail without a changeset.

## Architecture

The CLI uses a **two-phase argument parsing** strategy:

1. Parse argv to extract the service name (e.g., `drive`)
2. Fetch the service's Discovery Document, build a dynamic `clap::Command` tree, then re-parse

### Workspace Layout

The repository is a Cargo workspace with two crates:

| Crate                          | Package                 | Purpose                                           |
| ------------------------------ | ----------------------- | ------------------------------------------------- |
| `crates/gws-rust-core/`     | `gws-rust-core`      | Publishable library — core types and helpers       |
| `crates/gws-rust/` | `gws-rust`  | Binary crate — the `gwsr` CLI                       |

#### Library (`crates/gws-rust-core/src/`)

| File             | Purpose                                                    |
| ---------------- | ---------------------------------------------------------- |
| `discovery.rs`   | Serde models for Discovery Document + async fetch/cache    |
| `services.rs`    | Service alias → Discovery API name/version mapping         |
| `error.rs`       | `GwsError` enum, exit codes, JSON serialization            |
| `validate.rs`    | Path/URL/resource validators, `encode_path_segment()`      |
| `client.rs`      | HTTP client with retry logic                               |

#### CLI (`crates/gws-rust/src/`)

| File                | Purpose                                                                  |
| ------------------- | ------------------------------------------------------------------------ |
| `main.rs`           | Entrypoint, two-phase CLI parsing, method resolution                     |
| `auth.rs`           | OAuth2 token acquisition via env vars, encrypted credentials, or ADC     |
| `credential_store.rs` | AES-256-GCM encryption/decryption of credential files                  |
| `auth_commands.rs`  | `gwsr auth` subcommands: `login`, `logout`, `setup`, `status`, `export`   |
| `commands.rs`       | Recursive `clap::Command` builder from Discovery resources               |
| `executor.rs`       | HTTP request construction, response handling, schema validation          |
| `schema.rs`         | `gwsr schema` command — introspect API method schemas                     |
| `logging.rs`        | Opt-in structured logging (stderr + file) via `tracing`                  |
| `timezone.rs`       | Account timezone resolution: `--timezone` flag, Calendar Settings API    |

## Demo Videos

Demo recordings are generated with [VHS](https://github.com/charmbracelet/vhs) (`.tape` files).

```bash
vhs docs/demo.tape
```

### VHS quoting rules

- Use **double quotes** for simple strings: `Type "gwsr --help" Enter`
- Use **backtick quotes** when the typed text contains JSON with double quotes:
  ```
  Type `gwsr drive files list --params '{"pageSize":5}'` Enter
  ```
  `\"` escapes inside double-quoted `Type` strings are **not supported** by VHS and will cause parse errors.

### Scene art

ASCII art title cards live in `art/`. The `scripts/show-art.sh` helper clears the screen and cats the file. Portrait scenes use `scene*.txt`; landscape chapters use `long-*.txt`.

## Input Validation & URL Safety

> [!IMPORTANT]
> This CLI is frequently invoked by AI/LLM agents. Always assume inputs can be adversarial — validate paths against traversal (`../../.ssh`), restrict format strings to allowlists, reject control characters, and encode user values before embedding them in URLs.

> [!NOTE]
> **Environment variables are trusted inputs.** The validation rules above apply to **CLI arguments** that may be passed by untrusted AI agents. Environment variables (e.g. `GWSR_CONFIG_DIR`) are set by the user themselves — in their shell profile, `.env` file, or deployment config — and are not subject to path traversal validation. This is consistent with standard conventions like `XDG_CONFIG_HOME`, `CARGO_HOME`, etc.

### Path Safety (`crates/gws-rust-core/src/validate.rs`)

When adding new helpers or CLI flags that accept file paths, **always validate** using the shared helpers:

| Scenario                               | Validator                                | Rejects                                                              |
| -------------------------------------- | ---------------------------------------- | -------------------------------------------------------------------- |
| File path for writing (`--output-dir`) | `validate::validate_safe_output_dir()`   | Absolute paths, `../` traversal, symlinks outside CWD, control chars |
| File path for reading (`--dir`)        | `validate::validate_safe_dir_path()`     | Absolute paths, `../` traversal, symlinks outside CWD, control chars |
| Enum/allowlist values (`--msg-format`) | clap `value_parser` (see `gmail/mod.rs`) | Any value not in the allowlist                                       |

```rust
// In your argument parser:
if let Some(output_dir) = matches.get_one::<String>("output-dir") {
    crate::validate::validate_safe_output_dir(output_dir)?;
    builder.output_dir(Some(output_dir.clone()));
}
```

### URL Encoding (`crates/gws-rust/src/helpers/mod.rs`)

User-supplied values embedded in URL **path segments** must be percent-encoded. Use the shared helper:

```rust
// CORRECT — encodes slashes, spaces, and special characters
let url = format!(
    "https://www.googleapis.com/drive/v3/files/{}",
    crate::helpers::encode_path_segment(file_id),
);

// WRONG — raw user input in URL path
let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
```

For **query parameters**, use reqwest's `.query()` builder which handles encoding automatically:

```rust
// CORRECT — reqwest encodes query values
client.get(url).query(&[("q", user_query)]).send().await?;

// WRONG — manual string interpolation in query strings
let url = format!("{}?q={}", base_url, user_query);
```

### Resource Name Validation (`crates/gws-rust/src/helpers/mod.rs`)

When a user-supplied string is used as a GCP resource identifier (project ID, topic name, space name, etc.) that gets embedded in a URL path, validate it first:

```rust
// Validates the string does not contain path traversal segments (`..`), control characters, or URL-breaking characters like `?` and `#`.
let project = crate::validate::validate_resource_name(&project_id)?;
let url = format!("https://pubsub.googleapis.com/v1/projects/{}/topics/my-topic", project);
```

This prevents injection of query parameters, path traversal, or other malicious payloads through resource name arguments like `--project` or `--space`.

### Checklist for New Features

When adding a new helper or CLI command:

1. **File paths** → Use `validate_safe_output_dir` / `validate_safe_dir_path`
2. **Enum flags** → Constrain via clap `value_parser` or `validate_msg_format`
3. **URL path segments** → Use `encode_path_segment()`
4. **Query parameters** → Use reqwest `.query()` builder
5. **Resource names** (project IDs, space names, topic names) → Use `validate_resource_name()`
6. **Write tests** for both the happy path AND the rejection path (e.g., pass `../../.ssh` and assert `Err`)

## PR Labels

Use these labels to categorize pull requests and issues:

- `area: discovery` — Discovery document fetching, caching, parsing
- `area: http` — Request execution, URL building, response handling
- `area: docs` — README, contributing guides, documentation
- `area: tui` — Setup wizard, picker, input fields
- `area: distribution` — Nix flake, npm packaging, GitHub Actions release workflow, install methods
- `area: auth` — OAuth, credentials, multi-account, ADC
- `area: skills` — AI skill generation and management

## Helper Commands (`+verb`)

Helpers are handwritten commands prefixed with `+` that provide value the schema-driven Discovery commands cannot: multi-step orchestration, format translation (e.g., Markdown → Docs JSON), or multi-API composition.

> [!IMPORTANT]
> **Do NOT add a helper that** wraps a single API call already available via Discovery, adds flags to expose data already in the API response, or re-implements Discovery parameters as custom flags. Helper flags must control orchestration logic — use `--params` and `--format`/`jq` for API parameters and output filtering.

See [`src/helpers/README.md`](crates/gws-rust/src/helpers/README.md) for full guidelines, anti-patterns, and a checklist for new helpers.

## Environment Variables

### Authentication

| Variable | Description |
|---|---|
| `GWSR_TOKEN` | Pre-obtained OAuth2 access token (highest priority; bypasses all credential file loading) |
| `GWSR_CREDENTIALS_FILE` | Path to OAuth credentials JSON (no default; if unset, falls back to encrypted credentials in `~/.config/gwsr/`) |
| `GWSR_KEYRING_BACKEND` | Keyring backend: `keyring` (default, uses OS keyring with file fallback) or `file` (file only, for Docker/CI/headless) |

| `GOOGLE_APPLICATION_CREDENTIALS` | Standard Google ADC path; used as fallback when no gwsr-specific credentials are configured |

### Configuration

| Variable | Description |
|---|---|
| `GWSR_CONFIG_DIR` | Override the config directory (default: `~/.config/gwsr`) |

### OAuth Client

| Variable | Description |
|---|---|
| `GWSR_CLIENT_ID` | OAuth client ID (for `gwsr auth login` when no `client_secret.json` is saved) |
| `GWSR_CLIENT_SECRET` | OAuth client secret (paired with `CLIENT_ID` above) |

### Sanitization (Model Armor)

| Variable | Description |
|---|---|
| `GWSR_SANITIZE_TEMPLATE` | Default Model Armor template (overridden by `--sanitize` flag) |
| `GWSR_SANITIZE_MODE` | `warn` (default) or `block` |

### Helpers

| Variable | Description |
|---|---|
| `GWSR_PROJECT_ID` | GCP project ID override for quota/billing and fallback for helper commands (overridden by `--project` flag) |

### Logging

| Variable | Description |
|---|---|
| `GWSR_LOG` | Log level filter for stderr output (e.g., `gwsr=debug`). Off by default. |
| `GWSR_LOG_FILE` | Directory for JSON-line log files with daily rotation. Off by default. |

All variables can also live in a `.env` file (loaded via `dotenvy`).
