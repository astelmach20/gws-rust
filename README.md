<h1 align="center">gwsr</h1>

**One CLI for every Google Workspace API, built for scripts and AI agents.**<br>
Drive, Gmail, Calendar, Sheets, Docs, Admin and any other Discovery API. JSON on stdout, structured errors on stderr, stable exit codes, and 151 generated agent skills.

<p>
  <a href="https://github.com/astelmach20/gws-rust/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/astelmach20/gws-rust/ci.yml?branch=main&label=CI" alt="CI status"></a>
  <a href="https://crates.io/crates/gws-rust"><img src="https://img.shields.io/crates/v/gws-rust" alt="crates.io version"></a>
  <a href="https://www.npmjs.com/package/gws-rust"><img src="https://img.shields.io/npm/v/gws-rust" alt="npm version"></a>
  <a href="https://github.com/astelmach20/gws-rust/blob/main/LICENSE"><img src="https://img.shields.io/github/license/astelmach20/gws-rust" alt="license"></a>
</p>

`gwsr` has no hard-coded command list. It reads Google's [Discovery Service](https://developers.google.com/discovery) at runtime and builds its command tree from it, so new API methods show up as soon as Google publishes them. Hand-written `+helper` commands cover the multi-step jobs (sending mail, uploading files, exporting documents, streaming events) that a single REST call can't do.

> [!NOTE]
> `gws-rust` is a community-maintained fork of [googleworkspace/cli](https://github.com/googleworkspace/cli), which is no longer maintained; see [NOTICE](NOTICE). It is **not** compatible with the original `gws` command: config, credentials, environment variables, flags and output all differ. Set it up fresh with `gwsr auth setup`. This is not an officially supported Google product.

> [!IMPORTANT]
> `gwsr` is pre-1.0. Breaking changes are listed in the [changelog](CHANGELOG.md).

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Commands](#commands)
- [Authentication](#authentication)
- [Output contract](#output-contract)
- [Making requests](#making-requests)
- [Confirmations](#confirmations)
- [Helper commands](#helper-commands)
- [Model Armor](#model-armor)
- [Configuration](#configuration)
- [Environment variables](#environment-variables)
- [Shell completions and man pages](#shell-completions-and-man-pages)
- [AI agent skills](#ai-agent-skills)
- [Security](#security)
- [Troubleshooting](#troubleshooting)
- [Development](#development)

## Install

| Method | Command |
|---|---|
| npm (Node 22+) | `npm install -g gws-rust` |
| Homebrew (macOS, Linux) | `brew install astelmach20/tap/gws-rust` |
| Cargo | `cargo install gws-rust --locked` |
| Nix | `nix run github:astelmach20/gws-rust -- --help` or `nix profile install github:astelmach20/gws-rust` |
| Release archive | download from [GitHub Releases](https://github.com/astelmach20/gws-rust/releases) (below) |

Every method installs a single binary named `gwsr`.

- **npm** installs one prebuilt binary through a per-platform package (`gws-rust-darwin-arm64`, `gws-rust-darwin-x64`, `gws-rust-linux-arm64`, `gws-rust-linux-x64`, `gws-rust-win32-x64`) listed as an optional dependency. Nothing is downloaded and no install script runs. Installing with `--omit=optional` leaves the launcher without a binary, and it exits with an error saying so. The Linux packages carry static musl binaries that run on any distribution.
- **Cargo** builds from source. The minimum supported Rust version is 1.89.
- **From a checkout:** `cargo install --path crates/gws-rust --locked`.

### Release archives and verification

Each release has `gws-rust-<version>-<target>.tar.gz` archives (`.zip` on Windows) for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `{x86_64,aarch64}-unknown-linux-gnu` and `{x86_64,aarch64}-unknown-linux-musl`. It also has `SHA256SUMS`, a Sigstore bundle `SHA256SUMS.sigstore.json`, and a CycloneDX SBOM `gws-rust-<version>.cdx.json`. Every archive has SLSA build-provenance and SBOM attestations.

```sh
version=X.Y.Z
target=aarch64-apple-darwin   # or x86_64-unknown-linux-musl, ...
base=https://github.com/astelmach20/gws-rust/releases/download/v$version
curl -fsSLO "$base/gws-rust-$version-$target.tar.gz"
curl -fsSLO "$base/SHA256SUMS"
curl -fsSLO "$base/SHA256SUMS.sigstore.json"

# 1. Build provenance: the archive was built by this repository's release workflow
gh attestation verify "gws-rust-$version-$target.tar.gz" --repo astelmach20/gws-rust

# 2. Signed checksums (keyless Sigstore signature from the tag's release workflow)
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/astelmach20/gws-rust/.github/workflows/release.yml@refs/tags/v$version" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum --check --ignore-missing SHA256SUMS   # macOS: shasum -a 256 --check --ignore-missing SHA256SUMS

tar -xzf "gws-rust-$version-$target.tar.gz"
install -m 0755 "gws-rust-$version-$target/gwsr" ~/.local/bin/gwsr
```

npm packages are published with npm provenance (`npm audit signatures`), and crates with crates.io Trusted Publishing. macOS binaries are not notarized and Windows binaries are not Authenticode-signed; verify them with the steps above.

## Quick start

```bash
gwsr auth setup                  # create a Cloud project and OAuth client with gcloud (or see "Manual OAuth client")
gwsr auth login                  # read-only access to Drive, Sheets, Gmail, Calendar, Docs, Slides, Tasks
gwsr drive files list --params '{"pageSize": 5}' --fields 'files(id,name)'
gwsr drive files list --page-items --jq '.name'             # every file name, one per line
gwsr calendar +agenda --week --format table
gwsr auth login --write -s gmail && gwsr gmail +send --to you@example.com --subject hi --body hello
```

## Commands

```text
gwsr <service> <resource> [sub-resource] <method> [--params JSON] [--json JSON] [flags]
gwsr <service> +<helper> [flags]
```

| Command | What it does |
|---|---|
| `gwsr <service> ...` | Call any method in the service's Discovery document |
| `gwsr schema <service>.<resource>.<method>` | Parameters, request/response schema, pagination and upload info for a method; `gwsr schema drive.File --resolve-refs` for a type |
| `gwsr commands [SERVICE]...` | JSON inventory of every service, resource, method and helper with its flags (`--format table\|csv` for one row per command) |
| `gwsr batch <service>` | Many calls in `multipart/mixed` batch requests (see [Batch](#batch)) |
| `gwsr auth ...` | `setup`, `login`, `status`, `list`, `use`, `export`, `logout` |
| `gwsr cache clear` | Delete cached Discovery documents |
| `gwsr completions <shell>` | Shell completion script |
| `gwsr help <path>` | Same as `<path> --help`, e.g. `gwsr help drive files list` |
| `gwsr dev generate-skills`, `gwsr dev man` | Maintainer tools: agent skills and man pages |

`--help` works at every level (`gwsr drive --help`, `gwsr drive files --help`, `gwsr drive files list --help`) and prints to stdout. Method help groups its flags under Request, Output, Pagination and Upload headings. Pagination, upload, `--wait` and `--yes` flags appear only on methods where they apply. Methods Google marks deprecated are hidden.

### Services

| Service | API | | Service | API |
|---|---|---|---|---|
| `drive` | Drive v3 | | `admin` (`directory`, `admin-directory`) | Admin SDK Directory `directory_v1` |
| `sheets` | Sheets v4 | | `admin-reports` (`reports`) | Admin SDK Reports `reports_v1` |
| `gmail` | Gmail v1 | | `datatransfer` (`admin-datatransfer`) | Admin SDK Data Transfer `datatransfer_v1` |
| `calendar` | Calendar v3 | | `alertcenter` | Alert Center v1beta1 |
| `docs` | Docs v1 | | `cloudidentity` | Cloud Identity v1 |
| `slides` | Slides v1 | | `groupssettings` | Groups Settings v1 |
| `tasks` | Tasks v1 | | `licensing` | Enterprise License Manager v1 |
| `people` | People v1 | | `reseller` | Reseller v1 |
| `chat` | Chat v1 | | `vault` | Vault v1 |
| `classroom` | Classroom v1 | | `driveactivity` | Drive Activity v2 |
| `forms` | Forms v1 | | `drivelabels` | Drive Labels v2 |
| `keep` | Keep v1 | | `chromemanagement` | Chrome Management v1 |
| `meet` | Meet v2 | | `chromepolicy` | Chrome Policy v1 |
| `script` | Apps Script v1 | | `postmaster` (`gmailpostmastertools`) | Postmaster Tools v2 |
| `events` | Workspace Events v1 | | `cloudsearch` | Cloud Search v1 |
| `modelarmor` | Model Armor v1 | | `workflow` (`wf`) | cross-service helpers only |

**Any other Discovery API** works as `<api>:<version>`, for example `gwsr youtube:v3 videos list` or `gwsr admin:directory_v1 users list`. `--api-version V` (or `--api-version=V`) overrides the version of a registered service. A mistyped service name gets a "did you mean" suggestion.

Discovery documents are cached for 24 hours under `<platform cache dir>/gwsr/discovery` (`~/Library/Caches/gwsr` on macOS, `~/.cache/gwsr` on Linux; override with `GWSR_CACHE_DIR`). Fetches retry on 5xx. If a fetch fails, an expired cached copy is used with a warning on stderr. A document is checked (name, version, trusted endpoints) before it is written to the cache. Cache files are `0600` in a `0700` directory, and a corrupt or tampered entry is discarded and fetched again.

## Authentication

### Which setup do I need?

| I have… | Use |
|---|---|
| `gcloud` installed and signed in | `gwsr auth setup`, then `gwsr auth login` |
| A Cloud project but no `gcloud` | [Manual OAuth client](#manual-oauth-client), then `gwsr auth login` |
| A machine without a browser (SSH, container) | `gwsr auth login --no-localhost` |
| A service account | `GWSR_CREDENTIALS_FILE=key.json`, plus `--impersonate` for domain-wide delegation |
| An access token from another tool | `GWSR_TOKEN` or `GWSR_TOKEN_FILE` |

### `gwsr auth setup`

Uses `gcloud` to pick or create a project, enable the Workspace APIs, and create a Desktop OAuth client, saved as `client_secret.json` in the config directory. On a terminal it shows an interactive screen. With `--non-interactive`, or without a terminal, it prints JSON that ends in `"status": "action_required"` plus the manual steps that remain. Flags: `--project ID`, `--login` (run `auth login` afterwards), `--dry-run`. If `gcloud` is missing, it lists the manual options.

### `gwsr auth login`

Signs in with the OAuth authorization-code flow: PKCE (S256), a random `state` that is checked, and a loopback redirect to `http://127.0.0.1:<port>/`. The browser opens automatically. stdout gets two JSON lines, so an agent can drive the flow:

```json
{"event":"authorization_url","url":"https://accounts.google.com/...","mode":"loopback","timeout_seconds":300,"next":"open the URL in a browser on this machine and approve"}
{"event":"login_complete","status":"success","profile":"default","account":"you@example.com","granted_scopes":[...],...}
```

**Scopes: read-only by default.** A plain `gwsr auth login` asks only for the read-only scopes of Drive, Sheets, Gmail, Calendar, Docs, Slides and Tasks. On a terminal, you can adjust them in an interactive picker first. Logins are **incremental**: scopes granted earlier are kept, and each profile records its granted scopes.

| Flag | Effect |
|---|---|
| `-s, --services drive,gmail` | Limit the request to these services (other services' scopes come from their Discovery documents) |
| `--write` | Read-write scopes for the core services |
| `--full` | Read-write core scopes plus `gmail.settings.basic`, Pub/Sub and `cloud-platform` |
| `--scopes a,b` | Exact scopes; short names such as `gmail.modify` expand to `https://www.googleapis.com/auth/gmail.modify` |
| `--no-browser` | Print the URL but don't open a browser |
| `--no-localhost` | Headless/SSH: open the URL on any machine, approve, then paste the redirected `http://127.0.0.1/...` URL back into the prompt (no local listener) |
| `--timeout SECS` | How long to wait for sign-in (default 300) |
| `--login-hint EMAIL` | Pre-select an account on the consent screen |

For each API call, `gwsr` picks the narrowest granted scope that the method accepts. It avoids partial-visibility scopes such as `drive.file` or `gmail.metadata` unless nothing else fits. When no granted scope fits, it warns on stderr, and the error hint gives the exact `gwsr auth login --scopes ...` command to fix it.

### Manual OAuth client

1. In the [Cloud Console](https://console.cloud.google.com/apis/credentials/consent), configure the OAuth consent screen (External; testing mode is fine) and add your account under **Test users**.
2. Create an OAuth client of type **Desktop app** and download its JSON.
3. Save it as `~/.config/gwsr/client_secret.json` (mode `0600`), or export `GWSR_CLIENT_ID` and `GWSR_CLIENT_SECRET`.
4. Enable the APIs you need in the project, then run `gwsr auth login`.

The OAuth client is resolved from `GWSR_CLIENT_ID` + `GWSR_CLIENT_SECRET` (set both or neither), then `client_secret.json`. A build can also compile in a default client through the build-time variables `GWSR_DEFAULT_CLIENT_ID` / `GWSR_DEFAULT_CLIENT_SECRET`. Official builds contain none.

### Profiles

Each profile has its own credentials, token cache and granted scopes under `profiles/<name>/`. The active profile is chosen by:

1. `--profile NAME` (a global flag, accepted anywhere on the command line)
2. `GWSR_PROFILE`
3. `profile` in `config.toml`, which `gwsr auth use NAME` writes
4. `default`

```bash
gwsr auth login --profile work --write
gwsr auth use work          # make it the default
gwsr auth list              # {"active_profile":"work","active_profile_source":"config",...,"profiles":[...]}
gwsr --profile personal gmail +triage
```

### Service accounts and domain-wide delegation

```bash
export GWSR_CREDENTIALS_FILE=/secure/service-account.json   # must not be group/other-readable
gwsr --impersonate alice@example.com gmail users messages list --params '{"userId":"me"}'
```

`--impersonate USER` (or `GWSR_IMPERSONATE`) works only with service-account keys. It needs domain-wide delegation for the service account's client ID in the Admin console. When that is missing, the `unauthorized_client` error explains the setup.

### Credential sources

Checked in this order:

1. `GWSR_TOKEN` (an access token) or `GWSR_TOKEN_FILE` (a file holding one)
2. `GWSR_CREDENTIALS_FILE` (authorized-user or service-account JSON)
3. The active profile's encrypted credentials (`gwsr auth login`)
4. `GOOGLE_APPLICATION_CREDENTIALS`
5. `~/.config/gcloud/application_default_credentials.json`

Once a source exists, it is used or the command fails. A damaged or undecryptable credentials file is never deleted or silently skipped. A profile you selected explicitly never falls back to Application Default Credentials. `--dry-run` loads no credentials at all.

### Storage and key backends

Refresh tokens and the access-token cache are encrypted with AES-256-GCM. The 256-bit key lives in exactly one place, chosen by `GWSR_KEYRING_BACKEND`:

| Backend | Key location | Use for |
|---|---|---|
| `keyring` (default) | OS credential store: macOS Keychain, Windows Credential Manager, Secret Service on Linux/BSD | Desktops. Keyring failures are errors, never a silent fallback |
| `file` | `<config dir>/encryption.key`, which must be a `0600` regular file | Containers, CI, headless Linux without Secret Service |

Keys are never copied between backends, and a new key is never generated while encrypted data exists. Any other backend value is an error.

```text
~/.config/gwsr/                 0700   (GWSR_CONFIG_DIR)
├── client_secret.json                 OAuth client, shared by all profiles
├── config.toml                        settings, including the default profile
├── encryption.key                     only with GWSR_KEYRING_BACKEND=file
└── profiles/<name>/            0700
    ├── credentials.enc                encrypted refresh token
    ├── token_cache.enc                encrypted access-token cache (cross-process locked)
    └── profile.json                   account and granted scopes (no secrets)
```

Credential, token and client files that are readable by group or others are refused. A corrupt token cache is moved aside to `.corrupt-<time>`, never deleted.

### Status, export and logout

| Command | Output |
|---|---|
| `gwsr auth status [--offline]` | Active profile and where it came from, credential source, key backend, granted scopes. `--offline` skips the network check |
| `gwsr auth export` | The profile's credentials with secrets masked |
| `gwsr auth export --unmasked --output FILE` | Real secrets, written to a new `0600` file. Asks for confirmation, or pass `--yes-i-understand-this-exposes-secrets` when stdin is not a terminal |
| `gwsr auth logout [--no-revoke]` | Revokes the refresh token at Google, then deletes only the files `gwsr` created. A file named by `GWSR_CREDENTIALS_FILE` is left in place and reported |

For CI, export once on a machine with a browser, then store the file as a secret and point `GWSR_CREDENTIALS_FILE` at it:

```bash
gwsr auth export --unmasked --output ci-credentials.json
```

## Output contract

`gwsr` is machine-readable by default, whether or not stdout is a terminal.

- **stdout carries only results.** JSON by default: pretty on a terminal, compact when piped. Paginated output and streams (`--page-all`, `--page-items`, `gmail +watch`, `events +subscribe`, `gwsr batch`) are NDJSON, one object per line. Empty and `204` responses print `{"status":"success","httpStatus":204}`. Text bodies are wrapped in a JSON object.
- **stderr carries everything else:** logs, warnings, progress and prompts.
- **Human formats are opt-in:** `--format table|yaml|csv` (or `GWSR_FORMAT`, or `format` in `config.toml`).
- The only non-JSON stdout is `--help`, `gwsr completions`, `--version`, and raw payloads you ask for explicitly (`-o -`, or `--output -` on helpers).

| Flag | Effect |
|---|---|
| `--format json\|table\|yaml\|csv` | Output format (default `json`) |
| `--jq EXPR` | Filter with a jq expression (built in via [jaq](https://github.com/01mf02/jaq)). String results print raw, like `gh --jq` |
| `--columns a,b.c` | Columns for `table`/`csv` (dot paths for nested fields). An unknown column is an error |
| `--compact` / `--pretty` | Force the JSON layout (`GWSR_JSON_STYLE=auto\|compact\|pretty`) |

Table output escapes terminal control characters. CSV cells that start with a formula character get a `'` prefix.

### Errors

On failure stdout is empty, and stderr gets exactly one JSON object:

```json
{"error":{"code":400,"message":"Invalid --params for drive.files.list:\n- unknown parameter 'pageSze' (did you mean 'pageSize'?)\nRun `gwsr schema drive.files.list` to see the accepted parameters.","reason":"validationError","retryable":false}}
```

The fields are `code`, `message`, `reason` and `retryable`, plus `hint` when there is an actionable fix (a missing scope, API not enabled, rate limit, re-login, `--yes`). For API errors that Google explains, `enable_url` is also included. With a human `--format`, the error is printed as text instead (`error[validation]: ...`).

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | API error: Google rejected the request (permanent; don't retry unchanged) |
| `2` | Auth error: credentials missing, expired or invalid |
| `3` | Validation error: bad arguments, flags, config or input |
| `4` | Discovery error: could not fetch or parse the API schema |
| `5` | Internal error: I/O, serialization, or other unexpected failure |
| `6` | Retryable API error: 429, 5xx, or 403 `rateLimitExceeded`/`userRateLimitExceeded`; retry with backoff |
| `7` | Confirmation required: a gated action ran without `--yes` and without a terminal, or the prompt was declined. Nothing was sent |

### Logging

stderr log lines are JSON objects, unless `-v` is given or the output format is human. The default level is warnings and errors. Precedence: `-v`/`-vv`/`-vvv` or `-q` > `GWSR_LOG` (a filter such as `gwsr=debug`) > `RUST_LOG` > `log` in `config.toml`. `GWSR_LOG_FILE` (or `log_file`) names a directory that receives daily `gwsr.log.YYYY-MM-DD` JSON files at debug level (directory `0700`, files `0600`). Progress messages appear only with `-v`.

## Making requests

### Parameters and bodies

```bash
gwsr drive files list --params '{"q": "name contains \"report\"", "pageSize": 10}'
gwsr sheets spreadsheets create --json '{"properties": {"title": "Q1 Budget"}}'
gwsr gmail users messages list --params @params.json      # @FILE reads a file
echo '{"title":"Groceries"}' | gwsr tasks tasklists insert --json -   # - reads stdin
```

- `--params` and `--json` are checked against Discovery before anything is sent: parameter names, types, enums, repeated values, required path parameters, and body fields. Unknown names are rejected with a "did you mean" suggestion, and with a hint when a name belongs in `--json`. `--allow-unknown-params` and `--allow-unknown-fields` send them anyway (for example, for Developer Preview fields).
- `--fields MASK` requests a partial response, for example `--fields 'files(id,name)'`. When paginating, `nextPageToken` is added automatically.
- `--dry-run` validates the request and prints it as JSON (`method`, `url`, `query_params`, `body`, `page_all`, `upload`) without loading credentials or sending anything.
- `supportsAllDrives=true` is sent by default on every method that has it, so Shared Drives just work.
- Path segments are percent-encoded. RFC 3986 unreserved characters (`- . _ ~`) and `@` are kept as-is.

### Sheets ranges and shell quoting

Ranges contain `!`, which interactive bash history-expands inside double quotes. Use single quotes: `--range 'Sheet1!A1:C10'`.

### Timeouts and retries

- `--timeout SECS` (or `GWSR_TIMEOUT`, or `timeout_secs` in `config.toml`; default 60, `0` disables) limits the wait for a response and each gap while reading the body. Uploads get longer limits scaled by size. The connect timeout is 10 seconds.
- Idempotent requests are retried (up to 4 attempts) with capped exponential backoff and full jitter on 408, 429, 500, 502, 503 and 504, on connection errors and timeouts, and on Google 403 `rateLimitExceeded`/`userRateLimitExceeded`. `Retry-After` is honored; if it asks for more than 60 seconds, `gwsr` stops retrying and returns that response.
- **Non-idempotent requests (POST/PATCH) are sent at most once**, unless the connection never opened or the request carries a `requestId`. A timed-out `gmail +send` is never re-sent.
- A `401` triggers one forced token refresh and a retry, for every command, including long-running streams.

### Pagination

| Flag | Effect |
|---|---|
| `--page-all` | Fetch every page, one JSON line per page (NDJSON) |
| `--page-items[=FIELD]` | Fetch every page, one JSON line per item. The list field is detected, or name it, e.g. `--page-items=files` |
| `--page-limit N` | Stop after N pages (default `0`, unlimited). A truncated run warns on stderr and ends with `{"_truncated":{"pagesFetched":N,"nextPageToken":"..."}}` |
| `--page-delay MS` | Delay between pages (default 100) |

`page_limit` and `page_delay_ms` in `config.toml` (or `GWSR_PAGE_LIMIT`, `GWSR_PAGE_DELAY_MS`) set the defaults. Methods that take the page token in the request body (for example `driveactivity activity query`) are handled automatically.

### Uploads

```bash
gwsr drive files create --json '{"name": "report.pdf"}' --upload ./report.pdf
gwsr drive files create --json '{"name": "big.zip"}' --upload ./big.zip --upload-content-type application/zip
```

Files over 5 MiB use the resumable protocol when the method supports it (`--upload-resumable` forces it). Chunks are 8 MiB, and after a failed chunk the upload asks the server how much it has and resumes, up to 5 consecutive failures. Progress is shown on stderr when stderr is a terminal.

### Downloads

```bash
gwsr drive files get --params '{"fileId": "ID", "alt": "media"}' -o report.pdf   # prints a JSON summary
gwsr drive files get --params '{"fileId": "ID", "alt": "media"}' -o - > report.pdf # raw bytes on stdout
gwsr gmail users messages attachments get --params '{"userId":"me","messageId":"M","id":"A"}' \
  -o invoice.pdf --decode-field data
```

- A binary response needs `-o PATH` or an explicit `-o -`. `gwsr` never writes binary data to stdout unasked, and never invents a file name.
- Files are written atomically (a temp file, then a rename) after `Content-Length` is checked. A `204` creates no file.
- Base64 fields are decoded into `-o` automatically for `format: byte` fields, or when you name one with `--decode-field`. Standard and URL-safe base64, padded or not, are accepted.
- `drive files download` follows the returned `downloadUri`, and waits for the operation automatically when `-o` is given.

File path flags (`-o`, `--upload`, `--output-dir`, `--dir`, `@file`) accept any path. Set `GWSR_RESTRICT_PATHS=cwd` to confine them to the current directory, which is useful for sandboxed agents. Symlinks are resolved and control characters are rejected either way.

### Long-running operations

`--wait` polls a returned long-running operation through the API's `operations.get` until it finishes, then prints its result. `--wait-timeout SECS` sets the limit (default 600). A failed operation is an error.

### Batch

`gwsr batch <service[:version]>` reads NDJSON calls from stdin or `--input FILE`. It sends them in `multipart/mixed` batches of up to 100, and writes one result line per call:

```bash
cat <<'EOF' | gwsr batch drive
{"id": "a", "method": "files.get", "params": {"fileId": "ID1", "fields": "id,name"}}
{"id": "b", "method": "files.get", "params": {"fileId": "ID2", "fields": "id,name"}}
EOF
# {"id":"a","status":200,"body":{...}}
# {"id":"b","status":404,"body":{"error":{...}}}
```

Every call is validated like a normal command (`--allow-unknown-params` and `--allow-unknown-fields` apply). Destructive calls need `--yes`. Failed calls are reported as result lines on stdout. When any call fails, the command exits `1` with reason `batchPartialFailure` after printing every result.

## Confirmations

One confirmation gate covers generated methods, `gwsr batch` and every helper.

| Class | What | Gated |
|---|---|---|
| **Destructive** | Any `DELETE` method; `gmail.users.messages.batchDelete`, `calendar.calendars.clear`, `people.people.batchDeleteContacts`, `tasks.tasks.clear`, `drive.files.emptyTrash`; `calendar +delete`, `sheets +clear`, `script +push`, `gmail +filter delete`, `admin +user-suspend`; `drive +share` for owner transfer or writer-level access for a whole domain or anyone with the link | Always |
| **Outbound** | Actions that notify or grant access to other people, or run code: `gmail +send/+reply/+reply-all/+forward` (not `--draft`), `gmail +unsubscribe`, `chat +send`, `drive +share`, `calendar +insert` with attendees, `+update`, `+rsvp`, `script +run`, `admin +group-add-member`, `admin +user-suspend --unsuspend`, `workflow +file-announce` | Only when `GWSR_REQUIRE_CONFIRM=1` |

A gated action proceeds without a prompt when `--yes`/`-y` or `--dry-run` is given. On a terminal it asks. Without a terminal it is refused with exit code `7` (`"reason":"confirmationRequired"`) and nothing is sent. Agents should ask the user, then re-run with `--yes`. `GWSR_REQUIRE_CONFIRM` accepts `1`/`true` or `0`/`false`; any other value makes a gated action fail with a validation error.

## Helper commands

Helpers are hand-written commands prefixed with `+`, so they never collide with Discovery method names. Run `gwsr <service> --help` to see them next to the API resources. They print JSON by default and honor `--dry-run` (printing the planned requests), `--format`, `--jq` and `--sanitize`.

| Service | Helpers |
|---|---|
| `gmail` | `+send`, `+reply`, `+reply-all`, `+forward` (all with `--attach`, `--html`, `--draft`, `--from`), `+read`, `+search`, `+triage`, `+label`, `+archive`, `+trash`, `+filter list\|create\|delete`, `+unsubscribe` (RFC 8058 one-click), `+resolve-url` (Gmail web URL to API ID), `+attachments`, `+watch` |
| `drive` | `+upload`, `+download`, `+export`, `+move`, `+share`, `+sync` (one-way mirror to a local directory, never deletes) |
| `docs` | `+create`, `+read` (Markdown or text), `+write` (text or Markdown as native formatting), `+replace` |
| `sheets` | `+append`, `+read` (CSV export with `--output`), `+write`, `+clear`, `+create` |
| `calendar` | `+insert`, `+agenda`, `+freebusy` (with `--slot` to find free time), `+update`, `+delete`, `+rsvp` |
| `chat` | `+send`, `+spaces`, `+read` |
| `script` | `+push`, `+pull`, `+run`, `+logs` |
| `admin` | `+user-create`, `+user-suspend`, `+group-add-member` |
| `admin-reports` | `+audit` |
| `tasks` | `+add`, `+list`, `+lists` |
| `people` | `+find` |
| `forms` | `+responses` |
| `slides` | `+create`, `+read` |
| `meet` | `+create` |
| `classroom` | `+courses` |
| `keep` | `+list` |
| `events` | `+subscribe`, `+renew` |
| `modelarmor` | `+sanitize-prompt`, `+sanitize-response`, `+create-template` |
| `workflow` | `+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce` |

```bash
gwsr gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice' --attach report.pdf
gwsr gmail +reply --message-id 18f1a2b3c4d --body 'Thanks!'
gwsr gmail +search --query 'from:billing newer_than:7d' --max 20
gwsr drive +upload --file ./report.pdf --folder-id FOLDER_ID --convert
gwsr drive +export --file-id DOC_ID --to pdf --output report.pdf
gwsr docs +write --document-id DOC_ID --text-file notes.md --markdown
gwsr sheets +append --spreadsheet-id SHEET_ID --range 'Sheet2!A1' --values 'Alice,95'
gwsr calendar +insert --summary 'Standup' --start '2026-06-17T09:00' --duration 30m --attendee bob@example.com
gwsr calendar +freebusy --start 2026-06-17 --duration 5d --slot 30m --working-hours 09:00-17:00
gwsr tasks +add --title 'File taxes' --due 2026-04-15
gwsr events +subscribe --target //chat.googleapis.com/spaces/SPACE_ID --event-types google.workspace.chat.message.v1.created --project my-project
```

Helper flag conventions: IDs are `--<noun>-id` (`--document-id`, `--spreadsheet-id`, `--calendar-id`, `--message-id`, `--space-id`, `--script-id`). People and groups are `--email`, `--user`, `--group`, `--member`. Local output is `--output PATH` (`-` for stdout) and never overwrites an existing file without `--overwrite`. Result counts use `--limit N`.

Calendar helpers interpret times without a UTC offset in `--timezone`, falling back to your Google account's time zone. It is fetched from the Calendar settings API and cached for 24 hours in the cache directory. A date-only `--start` creates an all-day event. `calendar +insert` emails attendees by default (`--send-updates all`).

`gmail +watch` and `events +subscribe` stream NDJSON on stdout. Transient failures (408/429/5xx/network) back off with jitter, up to `--max-failures N` consecutive failures (default 10). Pub/Sub messages are acknowledged only after they are written.

## Model Armor

[Model Armor](https://cloud.google.com/security/products/model-armor) can screen API responses for prompt injection and other unsafe content before they reach an agent:

```bash
gwsr gmail users messages get --params '{"userId":"me","id":"MSG"}' \
  --sanitize projects/P/locations/us-central1/templates/T
```

| Setting | Values |
|---|---|
| `--sanitize TEMPLATE` / `GWSR_SANITIZE_TEMPLATE` / `sanitize_template` | `projects/P/locations/L/templates/T` |
| `GWSR_SANITIZE_MODE` / `sanitize_mode` | `warn` (default) or `block`. Any other value is an error |

- **`warn`:** output is printed. A match or a sanitization failure adds a `_sanitization` annotation and a warning on stderr.
- **`block`:** fails closed. A match or a sanitization failure prints nothing on stdout and exits non-zero.
- The template name is parsed strictly, and the request always goes to `modelarmor.<location>.rep.googleapis.com`.

`--sanitize` applies to generated methods and to the helpers that return user content (for example `gmail +read`, `+search`, `+triage`, `+watch`, `events +subscribe`, and the app helpers). `gwsr modelarmor +create-template --preset jailbreak` creates a template from the built-in preset.

## Configuration

`~/.config/gwsr/config.toml` (or `$GWSR_CONFIG_DIR/config.toml`). Unknown keys and invalid values are errors. For every setting, a **command-line flag beats an environment variable, which beats the config file, which beats the default**.

```toml
format = "table"          # json | table | yaml | csv                 (GWSR_FORMAT, --format)
json_style = "auto"       # auto | compact | pretty                   (GWSR_JSON_STYLE, --compact/--pretty)
page_limit = 50           # default --page-limit                      (GWSR_PAGE_LIMIT)
page_delay_ms = 100       # default --page-delay                      (GWSR_PAGE_DELAY_MS)
timeout_secs = 60         # request timeout                           (GWSR_TIMEOUT, --timeout)
sanitize_template = "projects/p/locations/l/templates/t"            # (GWSR_SANITIZE_TEMPLATE, --sanitize)
sanitize_mode = "warn"    # warn | block                              (GWSR_SANITIZE_MODE)
profile = "work"          # default profile; written by `gwsr auth use` (GWSR_PROFILE, --profile)
log = "gwsr=info"         # stderr log filter                         (GWSR_LOG, RUST_LOG, -v/-q)
log_file = "/var/log/gwsr" # JSON log directory                        (GWSR_LOG_FILE)
```

## Environment variables

All are optional. `gwsr` never reads `.env` files; set variables in your shell or deployment config.

| Variable | Description |
|---|---|
| **Credentials** | |
| `GWSR_TOKEN` | Pre-obtained OAuth access token (highest priority) |
| `GWSR_TOKEN_FILE` | File containing an access token |
| `GWSR_CREDENTIALS_FILE` | Authorized-user or service-account JSON |
| `GWSR_PROFILE` | Credential profile (same as `--profile`) |
| `GWSR_IMPERSONATE` | Service accounts: user to act as (same as `--impersonate`) |
| `GWSR_CLIENT_ID`, `GWSR_CLIENT_SECRET` | OAuth client for `gwsr auth login` (set both) |
| `GWSR_KEYRING_BACKEND` | `keyring` (default) or `file` |
| **Locations** | |
| `GWSR_CONFIG_DIR` | Config directory (default `~/.config/gwsr`) |
| `GWSR_CACHE_DIR` | Cache directory, absolute path (default: platform cache dir + `/gwsr`) |
| **Requests** | |
| `GWSR_PROJECT_ID` | Quota/billing project sent as `x-goog-user-project`, and the default `--project` for Pub/Sub helpers |
| `GWSR_NO_QUOTA_PROJECT` | `1`: never send `x-goog-user-project` (same as `--no-quota-project`) |
| `GWSR_TIMEOUT` | Request timeout in seconds (default 60, `0` disables) |
| `GWSR_API_BASE_URL` | Trusted API endpoint override for private endpoints or recording proxies (`https`, or `http` on localhost only) |
| `GWSR_RESTRICT_PATHS` | `cwd`: confine file flags to the current directory |
| `GWSR_REQUIRE_CONFIRM` | `1`: also require confirmation for outbound actions |
| **Output** | |
| `GWSR_FORMAT` | Default output format |
| `GWSR_JSON_STYLE` | `auto`, `compact` or `pretty` |
| `GWSR_PAGE_LIMIT`, `GWSR_PAGE_DELAY_MS` | Pagination defaults |
| `GWSR_SANITIZE_TEMPLATE`, `GWSR_SANITIZE_MODE` | Model Armor defaults |
| `GWSR_LOG`, `GWSR_LOG_FILE` | stderr log filter; JSON log directory |

`GOOGLE_APPLICATION_CREDENTIALS` is honored as the standard ADC fallback. `GWSR_COMPLETE` is set by the completion scripts; don't set it yourself.

## Shell completions and man pages

```bash
source <(gwsr completions bash)                 # ~/.bashrc
source <(gwsr completions zsh)                  # ~/.zshrc
gwsr completions fish | source                  # ~/.config/fish/config.fish
gwsr completions powershell | Out-String | Invoke-Expression
```

Completions are dynamic. They complete services, resources and methods from cached Discovery documents only, so a completion never hits the network. Supported shells are bash, zsh, fish, elvish and PowerShell. `gwsr dev man --output-dir man/` writes man pages for the static commands.

## AI agent skills

The repository ships 151 [Agent Skills](https://agentskills.io) (`skills/*/SKILL.md`), all generated from the command tree by `gwsr dev generate-skills`: one per service, one per helper, a shared reference (`gwsr-shared`), 10 personas and 41 recipes. See the [skills index](docs/skills.md).

```bash
npx skills add https://github.com/astelmach20/gws-rust                                   # all skills
npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/gwsr-gmail       # just one
ln -s "$(pwd)"/skills/gwsr-* ~/.openclaw/skills/                                      # OpenClaw
```

**Gemini CLI:** `gemini extensions install https://github.com/astelmach20/gws-rust` loads [CONTEXT.md](CONTEXT.md) as agent context. Sign in with `gwsr auth login` first; the extension uses the same credentials.

Agent-friendly habits: use `--fields` or `--jq` to keep responses small, `gwsr schema` before building a body, `--dry-run` before a mutation, and `--yes` only after the user has agreed. Branch on exit codes `6` (retry later) and `7` (ask the user).

## Security

- **Least privilege.** Login is read-only by default, and each call uses the narrowest granted scope.
- **Tokens only go to Google.** Credentials are attached only to `https://*.googleapis.com` (default port, no user info, query or fragment) or the origin you set in `GWSR_API_BASE_URL`. Discovery documents that point elsewhere are refused. Upload session and download URLs are checked too. HTTPS→HTTP redirects are refused, and `Authorization` is dropped on cross-host redirects.
- **Secrets at rest** are encrypted, with the key in the OS keyring (or a `0600` key file you opt into). Secrets are zeroized in memory, never printed by default, and permission-loose credential files are refused.
- **Process hardening.** At startup the umask is `077`, core dumps are disabled, and the process is marked non-dumpable on Linux. Log and output files are created `0600`. Both crates forbid `unsafe` code.
- **Untrusted input.** Arguments are treated as potentially adversarial (they often come from an LLM). Resource names are validated, path segments are encoded, and control characters are rejected. Terminal output is escaped, and CSV formula injection is neutralized.
- **Supply chain.** Dependencies are checked by `cargo deny` and `cargo audit` on every PR and daily. Releases are reproducible, signed and attested (see [verification](#release-archives-and-verification)). The npm package runs no install scripts.

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).

## Troubleshooting

| Symptom | Fix |
|---|---|
| "Access blocked" during login | Your OAuth app is in testing mode and your account isn't a test user. Add it under **OAuth consent screen → Test users** |
| "Google hasn't verified this app" | Expected for testing-mode apps. Choose **Advanced → Go to *app* (unsafe)** |
| Consent fails with many scopes | Unverified apps are limited to about 25 scopes. Request fewer with `-s drive,gmail` or `--scopes` |
| `redirect_uri_mismatch` | The OAuth client must be type **Desktop app** |
| No browser on this machine | `gwsr auth login --no-localhost` |
| Keyring errors on a headless Linux box or container | `export GWSR_KEYRING_BACKEND=file` |
| 403 `accessNotConfigured` | The API isn't enabled in your project. Open the `enable_url` from the error, enable it, and retry after a few seconds (or run `gwsr auth setup`) |
| 403 insufficient scopes | The error's `hint` has the exact `gwsr auth login --scopes ...` command. Logins are incremental |
| Exit code `7` | Re-run with `--yes` once you (or the user) agree, or use `--dry-run` to preview |
| Offline | `gwsr` uses cached Discovery documents, with a warning if they are stale. `gwsr cache clear` forces a refetch |

## Development

```bash
nix develop        # optional: the pinned toolchain plus just, cargo-deny, cargo-llvm-cov, ...
just               # list recipes
just ci            # the local CI gate: fmt, clippy, machete, shellcheck, actionlint, Rust and npm tests, cargo deny + audit
just coverage      # tests under cargo-llvm-cov with the 65% line-coverage floor
pnpm changeset     # every PR that changes Rust code needs a changeset
```

See [CONTRIBUTING](docs/CONTRIBUTING.md) and [AGENTS.md](AGENTS.md) for the architecture, coding standards and workflow.

## License

Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Derived from [googleworkspace/cli](https://github.com/googleworkspace/cli), Copyright Google LLC.

> [!CAUTION]
> This is **not** an officially supported Google product.
