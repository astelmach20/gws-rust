---
"gws-rust": minor
---

First release of `gws-rust`, the maintained fork of googleworkspace/cli. It does not read the old `gws` configuration, credentials or environment variables. Set it up fresh with `gwsr auth setup` and `gwsr auth login`.

**Breaking changes**

- **Names:** the binary is `gwsr`, the packages are `gws-rust` (crates.io, npm, Homebrew tap `astelmach20/tap/gws-rust`), environment variables use `GWSR_*`, and the config directory is `~/.config/gwsr`. `.env` files are no longer loaded.
- **Output:**
  - JSON is the default output everywhere, including helpers, `auth`, `setup`, `cache` and `commands`. Paginated output and streams are NDJSON. Human formats are opt-in with `--format table|yaml|csv`.
  - Errors are a single JSON envelope on stderr, and stdout is empty on failure.
  - Exit codes: 6 is a retryable API error (429/5xx/rate limit), and 7 means confirmation required.
- **Confirmations:** destructive actions (deletes, permanent deletes, destructive helpers) need `--yes`, or a prompt on a terminal. `GWSR_REQUIRE_CONFIRM=1` also gates outbound actions such as sending mail or sharing.
- **Helper flags:** IDs are `--<noun>-id` (`--document-id`, `--spreadsheet-id`, `--script-id`, `--space-id`, `--calendar-id`, `--message-id`, `--subscription-id`, `--tasklist-id`). `drive +upload` takes `--file` and `--folder-id`. `gmail +read` uses `--body-format`. Helper-local `--format`/`--dry-run` copies are gone; the global flags apply.
- **Requests:**
  - `--page-limit` defaults to unlimited, and a truncated run ends with a `_truncated` line.
  - Unknown `--params` and body fields are rejected; `--allow-unknown-params` / `--allow-unknown-fields` send them anyway.
  - Binary responses need `-o PATH` or `-o -`.
  - `GWSR_TIMEOUT_SECS` is replaced by `--timeout` / `GWSR_TIMEOUT`.
- **Auth:**
  - Login is read-only by default; `--write` and `--full` widen it, and `--readonly` is removed.
  - Credentials live in encrypted per-profile directories (`profiles/<name>/`), with the key in the OS keyring or, with `GWSR_KEYRING_BACKEND=file`, in `encryption.key`. There is no fallback between backends.
  - The plaintext `credentials.json` source is removed; use `GWSR_CREDENTIALS_FILE`.
  - `auth export` masks secrets unless `--unmasked --output FILE` is given.
  - `auth logout` revokes the token.
  - Credential files readable by group or others are refused.
- **Skills:** the generator is `gwsr dev generate-skills --output-dir DIR`, and skills are named `gwsr-*`.

**New**

- Profiles (`--profile`, `auth list`, `auth use`), service-account impersonation (`--impersonate`), `GWSR_TOKEN_FILE`, `auth login --no-localhost`, incremental scope grants and per-method least-privilege scope selection.
- `config.toml` (flag > env > config > default), `--jq`, `--columns`, `--compact`/`--pretty`, `-v`/`-q`, JSON stderr logs.
- `--fields`, `@file` and `-` for `--params`/`--json`, `--page-items`, resumable uploads with retry, `-o -` and `--decode-field`, `--wait` for long-running operations, `--no-quota-project`, `GWSR_API_BASE_URL`, `GWSR_RESTRICT_PATHS`.
- `gwsr batch`, `gwsr commands`, `gwsr cache clear`, `gwsr completions <shell>` (dynamic), `gwsr dev man`, and `<api>:<version>` for any Discovery API.
- Services: `admin` (Directory), `datatransfer`, `alertcenter`, `cloudidentity`, `groupssettings`, `licensing`, `reseller`, `vault`, `driveactivity`, `drivelabels`, `chromemanagement`, `chromepolicy`, `postmaster`, `cloudsearch`.
- Helpers:
  - gmail `+search`, `+label`, `+archive`, `+trash`, `+filter`, `+unsubscribe`, `+resolve-url`, `+attachments`;
  - drive `+download`, `+export`, `+move`, `+share`, `+sync`;
  - docs `+create`, `+read`, `+replace`, and Markdown for `+write`;
  - sheets `+read --output`, `+write`, `+clear`, `+create`, and `--range` for `+append`;
  - calendar `+update`, `+delete`, `+rsvp`, `+freebusy --slot`;
  - script `+pull`, `+run`, `+logs`;
  - helpers for `admin`, `admin-reports`, `tasks`, `people`, `chat` (`+spaces`, `+read`), `classroom`, `forms`, `keep`, `meet` and `slides`.
- Security: tokens are sent only to `*.googleapis.com` or the configured base URL; Discovery documents are validated; downloads are atomic; process hardening; terminal and CSV output escaping; Model Armor `block` mode fails closed.
- Distribution: signed and attested release archives (Sigstore, SLSA provenance, SBOM), per-platform npm packages with no install script, a Homebrew tap, and a Nix flake.
