# AGENTS.md

Guidance for anyone, human or agent, changing this repository. User-facing behavior is documented in [README.md](README.md). Agents that *use* the CLI should read [CONTEXT.md](CONTEXT.md) and the skills in `skills/`.

## Project overview

`gwsr` (package `gws-rust`) is a Rust CLI for every Google Workspace API. It builds its command tree at runtime from Google Discovery documents: there is no generated per-API client code.

> [!IMPORTANT]
> **Do not add generated `google-*` API crates.** `scripts/check-changeset.sh` fails CI if a crate manifest depends on one. To add a service, add a `ServiceEntry` to `SERVICES` in `crates/gws-rust-core/src/services.rs`. If its Discovery document is only served at `https://<api>.googleapis.com/$discovery/rest`, check that `DiscoveryLoader::discovery_urls` in `crates/gws-rust-core/src/discovery/mod.rs` finds it. Any other API already works as `<api>:<version>`.

## Project rules

These rules are not optional. Reviews reject code that breaks them.

1. **Best-practice security everywhere.** Treat every CLI argument as adversarial: it often comes from an LLM. See [Input validation](#input-validation-and-url-safety).
2. **No silent failures; fail loudly.**
   - No `let _ =` on a fallible result, and no `.ok()`, `unwrap_or_default()` or similar that hides an error.
   - No "warn and continue", unless continuing is genuinely correct *and* the warning is explicit.
   - Add context to errors: `thiserror` in the library, `anyhow::Context` in the binary.
   - No `unwrap()`/`expect()`/`panic!` in non-test code (clippy `unwrap_used`, `expect_used` and `panic` are denied in CI).
3. **No legacy or backward-compatibility shims.** No old environment variable aliases, deprecated flag aliases, config migrations or fallbacks "for compatibility". The project is pre-1.0: make clean breaks and record them in a changeset.
4. **Refactor freely.** Split large modules, and replace hand-rolled code with well-maintained crates. Upstream structure has no special standing.
5. **Modern tooling** at current versions (latest GitHub Actions pinned by SHA, `uv` rather than `pip`, current crates).
6. **Every behavior change gets focused automated tests**: unit tests, or integration tests with `wiremock` / `assert_cmd` / `insta`. Test the rejection path as well as the happy path.
7. **Machine-readable by default.**
   - Every command's stdout is JSON, whether or not it is a terminal. Streams are NDJSON.
   - Human formats (`table`, `yaml`, `csv`) are opt-in via `--format`.
   - Progress, prompts and logs go to stderr only.
   - Errors are one JSON envelope on stderr with a stable exit code (0–11; add a `GwsError` variant rather than reusing an unrelated code).
   - The only exceptions are `--help`, `--version`, `completions`, `dev man` and raw payloads the user asked for (`-o -`).
8. **Never touch real accounts in tests or development tooling.** No real OAuth logins or credentialed Google API calls. Unauthenticated public Discovery fetches are fine.

Use `pnpm` (pinned to 11.27.1 by `packageManager` in `package.json`), not `npm`, for the Node tooling: changesets, lefthook and the npm launcher tests.

## Development workflow

```bash
nix develop          # optional: pinned toolchain plus just, cargo-deny, cargo-audit, cargo-llvm-cov, cargo-machete, shellcheck, actionlint
pnpm install         # changesets CLI and lefthook (does not install git hooks)
pnpm run hooks       # optional: install the lefthook git hooks (refuses in a linked worktree)
just                 # list recipes
```

| Recipe | Runs |
|---|---|
| `just build` / `just release` | `cargo build` (debug / release) |
| `just fmt` / `just fmt-check` | rustfmt |
| `just clippy` | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` |
| `just test` | `cargo test --workspace --all-targets --all-features --locked` |
| `just test-js` | npm launcher tests |
| `just coverage [--open\|--lcov]` | `scripts/coverage.sh`: cargo-llvm-cov, fails below the line-coverage floor (65%, `COVERAGE_MIN_LINES`) |
| `just deny` | `cargo deny check` and `cargo audit --deny warnings` |
| `just machete`, `just shellcheck`, `just workflows` | Unused dependencies, shell lint, actionlint + zizmor |
| `just skills` | Regenerates `skills/` and `docs/skills.md` |
| `just ci` | `lint test test-js deny`: the local equivalent of the CI gate |

- **Toolchain:** Rust 1.98.1 is pinned in `rust-toolchain.toml`. The MSRV is 1.89 (CI checks it), the edition is 2024, and both crates forbid `unsafe` code.
- **Git hooks (lefthook, opt-in via `pnpm run hooks`):** pre-commit runs fmt, clippy, shellcheck and actionlint on staged files. Pre-push runs the tests, `cargo deny` and the JS tests. The install script refuses to run in a linked worktree, because all worktrees share one `.git/hooks`.
- **Snapshots:** CLI help and output snapshots live in `crates/gws-rust/tests/snapshots`. Review them with `cargo insta review`, or update them with `INSTA_UPDATE=always cargo test`.
- **Skills:** after changing help text, helper flags, `registry/*.toml` or the generator, run `just skills` and commit the result, including new and deleted directories. CI fails when `skills/` or `docs/skills.md` drift, and warns about skill directories without the generator marker. Every generated `SKILL.md` carries `<!-- gwsr generated skill: do not edit by hand -->` after its front matter. A full (unfiltered) run deletes marked directories it no longer produces (`pruned` in its summary); unmarked directories are never touched and are listed as `unmanaged`; delete those by hand if they are stale. `--dry-run` reports what would be written and pruned without touching disk.

### Changesets

Every PR that changes Rust code or a Cargo manifest needs a changeset (`pnpm changeset`, or write `.changeset/<name>.md` by hand):

```markdown
---
"gws-rust": minor
---

What changed, from the user's point of view. Call out breaking changes.
```

`gws-rust` is the only valid package name. Pre-1.0, use `minor` for features and breaking changes and `patch` for fixes. The release workflow turns pending changesets into a version PR and a `CHANGELOG.md` entry.

## Architecture

The repository is a Cargo workspace with two crates, plus the npm launcher in `npm/` and the Nix flake.

| Crate | Package | Purpose |
|---|---|---|
| `crates/gws-rust-core/` | `gws-rust-core` | Library: Discovery loading and caching, the service registry, the HTTP retry policy, validation, errors |
| `crates/gws-rust/` | `gws-rust` | The `gwsr` binary |

### Request flow

1. `main.rs` applies process hardening, loads `config.toml` and initializes logging. It then parses the static surface in `cli_args.rs`: global flags, `auth`, `schema`, `commands`, `batch`, `cache`, `completions`, `help`, `dev`.
2. Anything else is a service. `service.rs` resolves the name (aliases, `<api>:<version>`, `--api-version`), then loads the Discovery document through the cache. `commands.rs` builds a `clap::Command` tree from it, and the service's helper (`helpers::get_helper`) injects its `+verb` subcommands.
3. argv is parsed again against that tree. A helper handles the command, or the executor runs the generated method.
4. `confirm.rs` gates destructive and outbound actions. `auth` supplies a token for the narrowest granted scope. `transport` sends the request. Results go to stdout through `formatter.rs` / `output.rs`; errors become the stderr envelope in `error.rs`.

### Library (`crates/gws-rust-core/src/`)

| Module | Purpose |
|---|---|
| `discovery/` | `DiscoveryLoader`: fetch, trust checks, stale fallback. `cache.rs`: the on-disk cache (0600 files, atomic writes). `model.rs`: Serde types for Discovery documents |
| `services.rs` | `SERVICES` registry and `resolve_service()` (aliases, `<api>:<version>`) |
| `client.rs` | The shared `reqwest` client, redirect policy, `RetryPolicy`, `send()` with backoff/jitter/`Retry-After` and `Idempotency` |
| `validate/` | `mod.rs`: `encode_path_segment`, `validate_resource_name`, `validate_api_identifier`, `reject_dangerous_chars`. `paths.rs`: file-path policy (`GWSR_RESTRICT_PATHS`). `endpoint.rs`: which hosts may receive a token (`GWSR_API_BASE_URL`). `modelarmor.rs`: template-name parser |
| `error.rs` | `GwsError`, exit codes 0–11, the JSON error envelope, retryability |

### Binary (`crates/gws-rust/src/`)

| Module | Purpose |
|---|---|
| `main.rs` | Entry point and two-phase dispatch |
| `cli_args.rs` | Static command surface (clap derive), global flags, top-level help |
| `args.rs` | Fallible, typed accessors for parsed flags: the only way commands read arguments (a misdefined argument is an internal error, exit 5, never a panic or a silently absent value) |
| `output_file.rs` | Atomic writes of user-requested output files (`-o`, exports, pulled sources), honoring `--overwrite` |
| `config.rs` | `config.toml` loading and flag > env > config > default resolution |
| `service.rs`, `commands.rs` | Discovery document to `clap::Command` tree, method flags |
| `executor/` | Generated-method execution: `input.rs` (`--params`/`--json` validation), `body_schema.rs`, `url.rs`, `options.rs`, `pagination.rs`, `upload.rs` (multipart and resumable), `download.rs`, `operation.rs` (`--wait`), `batch.rs` (`gwsr batch`), `output.rs` (the executor's only stdout path) |
| `transport/` | The one credential-attaching request path: token, quota project, endpoint trust, 401 refresh-and-retry |
| `auth/` | `credentials.rs` (source resolution), `profiles.rs` (config dir layout), `keystore.rs` (AES-256-GCM, keyring/file backends), `token.rs`, `token_cache.rs`, `flow.rs` (PKCE loopback / `--no-localhost`), `scopes.rs` (scope sets and per-method selection), `client_config.rs`; `commands/` (`login`, `status`, `list`/`use`, `export`, `logout`, scope `picker`); `setup/` (`gwsr auth setup` with gcloud, TUI) |
| `confirm.rs` | The single confirmation gate (`Impact::Destructive`/`Outbound`, `--yes`, `GWSR_REQUIRE_CONFIRM`) |
| `formatter.rs`, `output.rs`, `jq.rs` | Output formats, terminal sanitization, `emit()`, `--jq` via jaq |
| `schema.rs`, `inventory.rs` | `gwsr schema`, `gwsr commands` |
| `completions.rs` | Dynamic shell completions and `dev man` |
| `generate_skills.rs` | `gwsr dev generate-skills` (reads `registry/personas.toml`, `registry/recipes.toml`) |
| `logging.rs`, `hardening.rs`, `fs_util.rs`, `timezone.rs`, `text.rs` | Logging, process hardening, secret-file I/O, account time zone, text utilities |
| `helpers/` | `+verb` helpers per service. `http.rs` has `Api`, the helper REST client (dry-run planning, pagination, sanitization) and `OutputTarget`. Gmail is in `helpers/gmail/`, Workspace Events in `helpers/events/` |

Tests: unit tests sit next to the code; CLI integration tests are in `crates/gws-rust/tests/{cli.rs,helpers_cli.rs}` (assert_cmd, insta, wiremock); property tests are in `crates/gws-rust-core/tests/validate_props.rs`.

## Input validation and URL safety

> [!IMPORTANT]
> Assume every CLI argument is adversarial. Reject control characters, constrain enums, encode values before putting them in URLs, and never send a token to an unchecked host.

Environment variables and `config.toml` are set by the user, so they are trusted input. They are still parsed strictly: an invalid value is an error, never ignored.

| Scenario | Use |
|---|---|
| Any HTTP request with credentials | `transport::Transport` (helpers: `helpers::http::Api`). Never build a `reqwest` request with a bearer token by hand |
| Value in a URL path segment | `gws_rust_core::validate::encode_path_segment()` (`encode_path_preserving_slashes()` for `{+name}` templates) |
| Query parameters | `ApiRequest::query()` / reqwest `.query()`, never string interpolation |
| Resource names (project, space, topic, template) | `validate::validate_resource_name()` |
| Service or API identifiers | `validate::validate_api_identifier()` |
| Input file paths (`--upload`, `--file`, `@file`) | `validate::validate_safe_file_path()` |
| Output / directory paths (`--output-dir`, `--dir`) | `validate::validate_safe_output_dir()` / `validate_safe_dir_path()` |
| Single output file (`--output`, `-` for stdout) | `helpers::http::OutputTarget::parse()` (refuses to overwrite without `--overwrite`) |
| Reading flags | `crate::args` accessors; never `ArgMatches::get_one`/`get_flag` (they panic) or `try_get_*().ok()` (it hides errors) |
| Writing local files | `crate::output_file` for user output; `fs_util::atomic_write` for private config and credential files. Both rely on the `077` umask set in `hardening.rs` |
| Uploads from helpers | `Api::upload_resumable` (resumes after failed chunks, keeps the session on the request's origin) |
| Enum flags | clap `value_parser` / `PossibleValuesParser` |
| Free text shown on a terminal | `output::sanitize_for_terminal()` |
| API base URLs | `validate::validate_api_base()` (enforces the token-host policy) |
| Destructive or outbound actions | `confirm::confirm(matches, Impact::…, "what will happen")`, and register `-y/--yes` with `confirm::with_yes()` |

Path validators apply `GWSR_RESTRICT_PATHS`. By default any path is allowed; `cwd` confines paths to the current directory. Either way they resolve symlinks and reject control characters.

## Helper commands (`+verb`)

Helpers are hand-written commands prefixed with `+`. They exist only when they add something the generated commands can't do: multi-step orchestration, format translation (Markdown to Docs, MIME for Gmail), or composing several APIs.

> [!IMPORTANT]
> **Do not add a helper that** wraps a single API call Discovery already exposes, adds flags for data already in the response, or re-implements Discovery parameters as custom flags. Output filtering is `--jq` / `--format`'s job.

See [`crates/gws-rust/src/helpers/README.md`](crates/gws-rust/src/helpers/README.md) for flag naming, the `Helper` trait and a checklist.

## Demo recordings

`docs/demo.tape` is a [VHS](https://github.com/charmbracelet/vhs) script (`vhs docs/demo.tape`). In `Type` strings, use double quotes for plain text and backticks when the text contains JSON; VHS does not support `\"`. Title cards live in `art/` and are shown with `scripts/show-art.sh`.

## PR labels

`area: discovery`, `area: http`, `area: auth`, `area: tui`, `area: skills`, `area: docs`, `area: distribution` (Nix, npm, release workflows, install methods).
