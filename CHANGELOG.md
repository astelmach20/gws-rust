# gws-rust

> Entries from 0.22.5 down are the history of the upstream project,
> [googleworkspace/cli](https://github.com/googleworkspace/cli), from which gws-rust was forked.

## 0.23.1

### Patch Changes

- 6efaedb: The `quota_project_id` from gcloud Application Default Credentials is now sent as `x-goog-user-project` only when ADC is the credential in use. With a `gwsr` profile, `GWSR_CREDENTIALS_FILE` or `GWSR_TOKEN`, and no `GWSR_PROJECT_ID` or client `project_id`, no quota project is sent instead of an unrelated gcloud project (which failed with a 403 naming that project).
- 10a7b35: `admin-reports +audit --since` no longer crashes (exit 101) on a value whose last character is not ASCII (such as `7é`) or on a look-back too large for the calendar (such as `99999999999999d`). These values are now rejected as validation errors (exit 3).
- b7220b6: Breaking: `admin +user-suspend` no longer has a `--reason` flag. The reason was sent as the user's `suspensionReason`, which the Directory API marks output-only and ignores, so the account never recorded it, even though the help said it did. Passing `--reason` is now a usage error instead of being silently dropped.
- e2502e2: `calendar +agenda` no longer reports every per-calendar failure as an internal error (exit 5). A calendar that cannot be read now keeps its documented exit code (1 for API errors, 2 for auth, 6 for 429/5xx rate limits, 10 for network errors), with the calendar name still in the message.
- bdf9465: `calendar +agenda --calendar-id` now fails with a validation error naming every requested calendar ID that is not in your calendar list. Previously, when at least one requested ID matched, the others were silently dropped and the agenda looked complete.
- 663f5ad: An API call whose access token is still rejected with HTTP 401 after the automatic refresh now fails as an auth error: exit code `2` with `"reason":"authError"`, as the exit-code table documents for expired or invalid credentials. It used to exit `1` ("API error, don't retry unchanged"), which pointed agents at the request instead of at `gwsr auth login`. The server's message and reason are kept in the error message.
- d9f6650: API errors in Google's current error format now take their `reason` from the `google.rpc.ErrorInfo` entry in `details` instead of the coarse `status`. A 403 `RATE_LIMIT_EXCEEDED` now exits with the retryable code `6` (it was `1`, even though the request had already been retried as rate-limited), and a `SERVICE_DISABLED` error now includes `enable_url` and the "API not enabled" hint.
- 4b53388: `gwsr auth status` no longer reports `"authenticated": false, "verified": true` when Google's token endpoint cannot be reached. Without a response nothing was verified, so the report now matches `--offline`: `"authenticated": true, "verified": false`, with the network failure in `verification_error`. A credential that Google actually rejects is still reported with `token_error`.
- ea4ba73: `gwsr batch` now prints results in input order even when Google returns the parts of a batch response in a different order, matching each part to its call by Content-ID. Duplicate call ids are rejected, including a generated id (a line's position) that equals an explicit id on another line. Previously both results were labeled with the same id.
- 225a533: `gwsr batch` now honors `--sanitize` / `GWSR_SANITIZE_TEMPLATE`: every result body is screened by Model Armor before it is printed, and block mode fails closed exactly as for a single call. Previously batch ignored the setting and printed API content unscreened, even in block mode.
- 7ad5b2b: `gwsr batch` now rejects input lines with keys other than `id`, `method`, `params` and `json` (for example a misspelled `body` or `parms`), `json` on a method that takes no request body, and `upload` on any method. Previously those keys were silently dropped and the call was sent without them.
- 41e1d3b: `gwsr calendar +agenda --today` / `--tomorrow` and `gwsr workflow +standup-report` now cover the whole local calendar day, from local midnight to the next local midnight. On daylight-saving transition days the window used to be a fixed 24 hours, so it dropped the last hour of a 25-hour fall-back day and spilled an hour into the next day after a 23-hour spring-forward day. Days whose midnight does not exist (zones that spring forward at midnight) now start at the first valid local time instead of failing.
- ba88b91: `calendar +insert`, `+update` and `+freebusy` no longer crash on a huge `--duration` or `--slot` (for example `99999999999999999d`, or a length that ends past the latest representable date). They now fail with a validation error (exit 3).
- d998f64: `calendar +insert` and `calendar +update` now send the time zone with a local (offset-less) end time when the start time has an offset. Previously the end was sent as `"timeZone": null`, so the API rejected the request; this happened for `+insert --start <RFC 3339> --end <local time>` and for `+update --end <local time>` without `--start`, which reuses the event's current start.
- bf2efa2: A `client_secret.json` whose `client_id` or `client_secret` is empty or contains whitespace, control or non-ASCII characters (typically a space pasted with the value) is now rejected with an error naming the field and file, instead of being sent to Google and failing as `invalid_client` ("The OAuth client was not found"). The same values are refused when the file is written.
- cd67ebf: Security: `--format csv` now neutralises spreadsheet formulas that start after a stripped control or invisible character. Before, a value such as `<BEL>=HYPERLINK(...)` or `<zero-width space>@SUM(...)` in API data had the leading character removed but did not get the `'` prefix, so a spreadsheet would run it as a formula.
- a489080: Paginated `--format csv` and `--format table` output (`--page-all`, `--page-items`) now aligns every row to the header printed with the first page. Previously each page computed its own columns, so a later page whose items had a missing, extra or reordered field printed values under the wrong headers. The first page with rows fixes the columns (the `--columns` selection, or that page's fields); later rows get empty cells for missing fields, and a field that first appears on a later page is left out with a warning on stderr naming it (use `--columns` or `--format json` to keep it). Table widths also stay fixed across pages, and `--page-items` items render as table rows instead of one key/value block per item.
- 7f0c903: Tests no longer share one HTTP connection pool across Tokio runtimes. Each `#[tokio::test]` runs its own runtime, and a pooled connection is driven by the runtime that opened it, so wiremock-backed tests (Pub/Sub pull, Workspace Events renew, Calendar, Docs) intermittently failed with "dispatch task is gone: runtime dropped the dispatch task". The CLI itself runs one runtime and keeps its single shared client; `gws_rust_core::client::build_client()` builds an unshared client for callers that run more than one runtime.
- 653e816: Discovery methods declared at the top level of a document, outside any resource (for example `oauth2:v2`'s `tokeninfo`), are now commands (`gwsr oauth2:v2 tokeninfo`), schema entries (`gwsr schema oauth2:v2.tokeninfo`) and `gwsr batch` targets. They used to be dropped without any message. Their paths go through the same endpoint-safety checks as resource methods.
- 99b1f7c: `drive +sync` no longer drops a file whose name differs from a sibling only by letter case (for example `Report.txt` and `report.txt`). On case-insensitive file systems (the macOS and Windows defaults) both mapped to one local file, so the second was silently reported as `unchanged` or overwrote the first. Such names now count as a collision, so each is saved as `<file id>-<name>`, like other same-name siblings.
- f27ef9f: `drive +sync` no longer merges sibling Drive folders that share a name, or a folder and a file with the same name, into one local path (which lost files and reported them as "unchanged"). Files and sub-folders in a Drive folder now share one set of local names: when more than one item maps to the same local name, every one of them is saved as `{id}-{name}`, so the layout no longer depends on the order Drive lists items in. This changes local paths for same-named files: previously the first one listed kept the plain name. An earlier sync's plain-named copy of such an item is left in place and is no longer updated. If an ID-prefixed name still collides with another item's real name, `+sync` fails before downloading anything from that folder instead of overwriting a file.
- b2763dd: `events +subscribe` now deletes the Pub/Sub topic and subscription it just created if a later setup step fails, for example when the Workspace Events API rejects the subscription. Before, the command exited with an error and left both resources behind, even with `--cleanup`. Nothing publishes to them, and no reconnect hint named them. If the Workspace Events operation is still running, the resources are kept.
- a9722d8: `forms +responses --output` now guards its CSV against formula injection, like `--format csv` does: a response or question title starting with `=`, `+`, `-` or `@` gets a leading `'`, so a respondent's answer such as `=HYPERLINK(...)` no longer becomes a live formula when the exported file is opened in a spreadsheet. JSON output is unchanged.
- aa342e1: `forms +responses` no longer drops answers when two questions have the same title, which is common in multi-section forms, or when a question is titled like a fixed column such as `responseId`. The JSON rows are keyed by column header, so the later answer used to overwrite the earlier one. A repeated title now gets its question ID appended, for example `Comments (1a2b3c4d)`, in both the JSON and CSV headers.
- 25c0c07: Gmail helpers now handle RFC 5322 comments in addresses. A message whose sender uses the legacy `alice@example.com (Alice Smith)` form is replied to at `alice@example.com`, not at the invalid address `alice@example.com (Alice Smith)`. A comma inside a comment, as in `bob@example.com (Smith, Bob)`, no longer splits one recipient into two broken ones in `+reply-all` Cc lists or in `--to`/`--cc`/`--bcc`. Parentheses that are not comments, such as `Malo (Work) <malo@example.com>` or an emoticon in a display name, are kept as they are.
- 65a858d: Gmail helpers no longer corrupt display names that contain escaped quotes or backslashes. Replying to `"Bob \"The Builder\"" <bob@example.com>` used to send the reply to `"Bob \\\"The Builder\\\""`, so every round trip added more backslashes. The recipient now sees `Bob "The Builder"`. A display name like `"A" and "B"` is also no longer turned into `A" and "B`.
- cb7c847: Gmail `+reply`, `+reply-all` and `+forward` now read the original message's headers case-insensitively, as RFC 5322 requires. Messages whose sender wrote headers such as `CC:`, `Reply-to:` or `Message-id:` previously lost their Cc recipients on reply-all, ignored the Reply-To address, or failed with "Message is missing Message-ID header"; they are now handled like any other message.
- c7e1234: Gmail `+read`, `+reply`, `+reply-all` and `+forward` no longer treat a whitespace-only `text/plain` alternative as the message body when the message also has an HTML part: the HTML part is rendered as text instead (links kept as footnotes), so `+read` no longer prints an empty body for such messages. When a message has no text or HTML body part at all, the fallback to the API snippet (which Gmail truncates) now logs a warning on stderr.
- c9deebd: `gmail +reply`, `+reply-all` and `+forward` now parse the original's `References` and `Message-ID` headers as RFC 5322 message-ID lists. `<a@x><b@x>` (no space between IDs) was read as one ID, and a `Message-ID` followed by a comment such as `<id@x> (added by relay)` produced a malformed `In-Reply-To: <<id@x> (added by relay)>`, which breaks threading in the recipients' mail clients (debug builds panicked on both).
- e328242: `gmail +reply-all` and `+reply` now understand RFC 5322 group syntax in the original message's To, Cc and Reply-To headers. Replying all to a message addressed to `undisclosed-recipients:;` (any Bcc-only message) no longer adds a bogus `<undisclosed-recipients:;>` Cc recipient, and members of a named group such as `Team: a@example.com, b@example.com;` keep their correct addresses instead of being mangled into `Team: a@example.com` and `b@example.com;`.
- 80521c6: `gmail +watch --label-ids` now emits only messages that carry one of the watched labels. Previously the label filter only decided which mailbox changes triggered a notification; every message added since the last notification was then emitted, including sent mail, drafts and messages under other labels.
- f1104f9: Floating-point numbers in API responses are now printed exactly as the API sent them. Before, many 17-significant-digit doubles, such as coordinates or unformatted Sheets values, were read slightly wrong and printed with a changed last digit (for example `13.346133595589677` became `13.346133595589675`) in every output format.
- 7aeba90: `gwsr auth login` now requests exactly the scopes you chose:
  
  - `--services` no longer keeps `cloud-platform` (for example with `--full -s gmail`), and the scope picker no longer lists it as belonging to every service. Request it with `--scopes cloud-platform` or plain `--full` if you need it.
  - The picker's "Full access" preset no longer adds `cloud-platform` when it is not a listed, checked row.
  - When `--services` names a service the picker has no scopes for (for example `-s chat` without `gwsr auth setup`), the picker is skipped and that service's scopes come from its Discovery document instead of being dropped.
  - An unknown `--services` name (such as `calendar.events`), or a service whose scopes cannot be looked up, is now an error instead of a warning.
- 4a4ac20: `gwsr auth login` now rejects `-s/--services` combined with `--scopes` (exit 3) instead of silently ignoring `-s`. `-s` filters the presets (`--write`, `--full`, the read-only default and the picker); `--scopes` is an exact list. To add scopes for more services, run another `gwsr auth login` (logins are incremental).
- b32a19f: `alt=media` downloads with `-o PATH` or `-o -` now deliver the file's exact bytes even when it is JSON. Previously a stored JSON file was parsed and re-serialized (keys reordered, numbers and whitespace rewritten). A JSON file with `name` and `metadata` keys was polled as a long-running operation, and a file that was not one valid JSON document (for example NDJSON) failed to download.
- eca3d4b: `gwsr auth login` no longer stalls for about 10 seconds after you approve access in the browser. The local callback listener now serves connections concurrently (up to 16 at once), so an idle connection the browser opens ahead of time can no longer hold up the real redirect.
- d4667b8: `--page-all` (and `--page-items`) no longer loops forever when an API returns a `nextPageToken` it has already returned. The pages fetched so far are still printed, then the command fails with an error that names the repeated token, instead of sending requests without end under the default unlimited `--page-limit`.
- f7a42cc: `--params`, `--json` and `gwsr batch` input lines now reject JSON objects that repeat a key, such as `{"fileId":"a","fileId":"b"}`, with a validation error (exit `3`) that names the key. Previously the last value silently won, so an ambiguous argument could target a different resource than intended, for example in a delete.
- 1b843bd: `--page-all` (and `--page-items`) now adds `nextPageToken` to a `fields` mask passed in `--params`, the same way `--fields` already did. Before this change, `--params '{"fields":"files(id)"}' --page-all` stopped after the first page because the API left out the token, and the command still exited 0.
- 89d0aeb: A 403 caused by the quota project sent as `x-goog-user-project` (the caller lacks `serviceusage.services.use` on it, for example because the OAuth client belongs to a project the user is not a member of) now carries a hint pointing at `--no-quota-project` / `GWSR_NO_QUOTA_PROJECT=1` and `GWSR_PROJECT_ID`.
- 733c43d: `GWSR_REQUIRE_CONFIRM=1` now also gates the generated methods that send mail or messages, share files or run scripts (`gmail users messages send`, `gmail users drafts send`, `chat spaces messages create`, `drive permissions create`/`update`, `script scripts run`, `admin members insert`), share a calendar (`calendar acl insert`/`update`/`patch`), give others access to or route mail out of a mailbox (`gmail users settings delegates create`, `updateAutoForwarding`, `forwardingAddresses create`, `sendAs create`, `filters create`), or email event attendees (`calendar events insert`/`update`/`patch`/`move`/`quickAdd` with `sendUpdates` `all`/`externalOnly` or `sendNotifications=true`; without those parameters Calendar sends no notifications, so the call is not gated), both directly and inside `gwsr batch`. Previously only the `+` helpers were gated, so the policy could be bypassed by calling the raw method. These methods now accept `-y/--yes`.
- 9844758: An empty path parameter, such as `--params '{"fileId":""}'`, is now a validation error (exit 3). Before this change it shortened the URL to the parent collection: `drive files delete` sent `DELETE .../drive/v3/files/`, and `gmail users messages get` requested `.../users/me/messages/`.
- d33a8e1: `gmail +reply` to a message you sent now goes to that message's original To recipients, as in Gmail, instead of back to yourself. A message counts as yours when its From matches your primary address or the `--from` alias, and Reply-To is ignored in that case. Replying to a note you sent only to yourself still goes to you. `+reply` now also reads your Gmail profile address to detect this.
- fae337c: `GWSR_RESTRICT_PATHS=cwd` no longer accepts a path such as `link/missing/../file` where `link` is a symlink pointing outside the current directory. `..` is now applied after symlinks in the existing prefix are resolved, so these paths are rejected (and, without the restriction, resolve to the symlink's real target).
- 4e2e60a: `drive +download`, `drive +export` and `drive +sync` no longer fail with "File name too long" on Drive files with long non-ASCII names (emoji, CJK, accented text). Local names derived from Drive names are now capped at 200 bytes rather than 200 characters; previously one such file aborted the whole `+sync`, leaving the remaining files undownloaded.
- 3baf51b: Method `--help` ("Full request/response schema: gwsr schema …") and the `Invalid --params` error now name a `gwsr schema` path that works. They used to print the Discovery method id, which fails for `events` (`workspaceevents.…`), `groupssettings` (`groupsSettings.…`), `alertcenter` / `cloudsearch` methods whose id skips a resource (`alertcenter.getSettings`), and every `<api>:<version>` service (`youtube.videos.list` instead of `youtube:v3.videos.list`).
- 866c674: `script +pull` keeps dots in Apps Script file names: `config.dev` is now saved as `config.dev.gs` instead of `config.gs`. Before, two files such as `config.dev` and `config.prod` were written to the same local file (the second overwrote the first with `--overwrite`, or failed after a partial write without it), and `+push` renamed them on the server.
- 63bc5b0: Skills installed one at a time now say what they depend on. Every service and helper skill lists `gwsr-shared` under `metadata.openclaw.requires.skills` and its prerequisite line gives the `npx skills add` command for `gwsr-shared`; recipe and persona skills say how to install any listed skill that is missing. The README's selective-install example installs `gwsr-shared` first.
- 8e03636: `gwsr auth status` now reports the OAuth client that `gwsr auth login` would use. When `GWSR_CLIENT_ID`/`GWSR_CLIENT_SECRET` are set, `client.source` is `environment_variables` and `client.overrides` names the `client_secret.json` they shadow (previously the file was reported as if it were in use). A half-set pair is reported as `client_error`.
- 683b6ab: `--sanitize` now screens text responses of generated methods (for example `drive files export` to `text/plain`, or a text file fetched with `alt=media`) that are printed inline as JSON. Previously only JSON responses were screened, so text content reached stdout unscreened even in block mode. Bodies saved with `-o PATH` or streamed with `-o -` are unchanged.
- 37023e9: The cached account time zone is now kept per identity (profile, credentials file, ADC file and `--impersonate` user) and is never cached for `GWSR_TOKEN`/`GWSR_TOKEN_FILE`. Previously one global cache file was shared, so after switching `--profile`, `gwsr auth use`, `GWSR_CREDENTIALS_FILE` or the `--impersonate` subject, calendar and workflow helpers used the previous account's time zone for up to 24 hours (wrong "today" window for `+agenda`/`+standup-report`, wrong event times for `+insert` without an offset). Existing cache files from older versions are ignored and removed on the next `gwsr auth login`/`logout`.
- 9a696d8: Access tokens are now cached per grant, not only per OAuth client. After `gwsr auth login`, a command that was already running with the previous login's refresh token could cache that grant's access token, and later commands would use it (possibly for another account or scope set). The cache key now includes a short SHA-256 fingerprint of the refresh token (never the token itself), so such an entry is never served to the new login. Tokens cached before this change are ignored and expire on their own.
  
  The cross-process lock also now notices when its lock file was deleted while waiting (as `gwsr auth logout` does): it relocks the current file, or fails with an error if the profile directory is gone, instead of holding a lock that excludes nobody.
- fa88a54: `gwsr auth login` and `gwsr auth logout` now clear the access-token cache while holding its cross-process lock. Previously a concurrent `gwsr` process that was refreshing a token could write the previous grant's access token back after the cache was cleared, so a later command could use a token for the old account or scope set. `auth logout` now also lists the cache lock file in `removed`.
- d8a3b9b: A 429 or 5xx from Google's OAuth token endpoint (refreshing a login, a service-account token, or the login code exchange) now exits `6` (retryable, reason `tokenEndpointUnavailable`) instead of `2` ("sign in again"). The credentials were not rejected, so agents should retry with backoff rather than ask the user to log in. The error keeps only a short excerpt of the endpoint's response body.
- b2aac81: `--profile NAME` together with `GWSR_TOKEN` or `GWSR_TOKEN_FILE` is now a configuration error (exit `8`), as it already was with `GWSR_CREDENTIALS_FILE`. Previously the explicit profile was silently ignored, and the command ran as whatever identity the access token belonged to, even if that profile did not exist. Unset one of them. Without `--profile`, the token still takes priority.
- 143739c: Add regression tests for behaviors reported upstream: a POST with no body sends `Content-Length: 0` (`drive files download` no longer fails with 411), `calendar +agenda` reports all-day dates unshifted in time zones east of UTC, and `tasks +list --show-completed` also requests hidden tasks. No behavior change.
- ce96570: A failed long-running operation (`--wait`, `events +subscribe`) now reports the HTTP status for its gRPC error code, and names the gRPC status in the message. Previously the raw gRPC code was reported as if it were an HTTP status (`"code": 14`), so a transient `UNAVAILABLE` or `RESOURCE_EXHAUSTED` failure exited with `1` ("don't retry") instead of `6`.
- e65d2c8: `GWSR_API_BASE_URL` now applies to the `workflow` helpers and to the account time zone lookup used by calendar and workflow helpers. Before, these always sent their requests (with the bearer token) to the public Google hosts, bypassing the configured private endpoint or recording proxy.
- 21cd984: `workflow` helpers (`+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`) now honor `--sanitize` / `GWSR_SANITIZE_TEMPLATE`. Previously they ignored the Model Armor configuration and printed calendar, Gmail and Drive content unsanitized, even in `block` mode.

## 0.23.0

### Minor Changes

- c391009: First release of `gws-rust`, the maintained fork of googleworkspace/cli. It does not read the old `gws` configuration, credentials or environment variables. Set it up fresh with `gwsr auth setup` and `gwsr auth login`.
  
  **Breaking changes**
  
  - **Names:** the binary is `gwsr`, the packages are `gws-rust` (crates.io, npm, Homebrew tap `astelmach20/tap/gws-rust`), environment variables use `GWSR_*`, and the config directory is `~/.config/gwsr`. `.env` files are no longer loaded.
  - **Output:**
    - JSON is the default output everywhere, including helpers, `auth`, `setup`, `cache` and `commands`. Paginated output and streams are NDJSON. Human formats are opt-in with `--format table|yaml|csv`.
    - Errors are a single JSON envelope on stderr, and stdout is empty on failure.
    - Exit codes run 0–11: 6 is a retryable API error (429/5xx/rate limit), 7 confirmation required, 8 configuration error, 9 credential store error, 10 network error (no response; not marked retryable), 11 output blocked by Model Armor. Internal argument errors exit 5, and an unknown `--format` exits 3.
  - **Confirmations:** destructive actions (deletes, permanent deletes, destructive helpers) need `--yes`, or a prompt on a terminal. `GWSR_REQUIRE_CONFIRM=1` also gates outbound actions such as sending mail or sharing.
  - **Helper flags:** IDs are `--<noun>-id` (`--document-id`, `--spreadsheet-id`, `--script-id`, `--space-id`, `--calendar-id`, `--message-id`, `--subscription-id`, `--tasklist-id`). `drive +upload` takes `--file` and `--folder-id`. `gmail +read` uses `--body-format`. Helper-local `--format`/`--dry-run` copies are gone; the global flags apply.
  - **Requests:**
    - `--page-limit` defaults to unlimited, and a truncated run ends with a `_truncated` line.
    - Unknown `--params` and body fields are rejected; `--allow-unknown-params` / `--allow-unknown-fields` send them anyway.
    - Binary responses need `-o PATH` or `-o -`.
    - `GWSR_TIMEOUT_SECS` is replaced by `--timeout` / `GWSR_TIMEOUT`.
    - Every environment variable `gwsr` reads is validated at startup, before any command runs and whether or not the command uses it. An invalid value (including one that isn't valid UTF-8) is a configuration error (exit 8) naming the variable and the accepted values; before, some were accepted silently until a command used them and others failed with exit 3. `RUST_LOG`/`GWSR_LOG` must be valid filters with module-path targets, `GWSR_CONFIG_DIR`/`GWSR_LOG_FILE` must be absolute, and boolean variables all accept `1/true/yes/on` and `0/false/no/off`.
    - An unknown `GWSR_*` environment variable is a configuration error (exit 8) with a "did you mean" suggestion, since it is almost always a typo or a removed name (`GWSR_TIMEOUT_SECS` suggests `GWSR_TIMEOUT`). Unset stale variables; there are no aliases.
    - A quota-project source that exists but can't be used (unreadable OAuth client config or ADC file) is an error instead of a warning; set `GWSR_PROJECT_ID` or pass `--no-quota-project`.
    - Output files (`-o`, exports, pulled sources) are written atomically and get the normal mode for the process umask. `gwsr` sets the umask to `077`, so they stay private to your user.
  - **Auth:**
    - Login is read-only by default; `--write` and `--full` widen it, and `--readonly` is removed.
    - Credentials live in encrypted per-profile directories (`profiles/<name>/`), with the key in the OS keyring or, with `GWSR_KEYRING_BACKEND=file`, in `encryption.key`. There is no fallback between backends.
    - The plaintext `credentials.json` source is removed; use `GWSR_CREDENTIALS_FILE`.
    - `auth export` masks secrets unless `--unmasked --output FILE` is given.
    - `auth logout` revokes the token.
    - Credential files readable by group or others are refused.
  - **Skills:** the generator is `gwsr dev generate-skills --output-dir DIR`, and skills are named `gwsr-*`. Generated `SKILL.md` files carry a marker, and a full run deletes the marked skills it no longer produces (`pruned`), listing unmarked directories as `unmanaged`.
  
  **New**
  
  - Profiles (`--profile`, `auth list`, `auth use`), service-account impersonation (`--impersonate`), `GWSR_TOKEN_FILE`, `auth login --no-localhost`, incremental scope grants and per-method least-privilege scope selection.
  - `config.toml` (flag > env > config > default), `--jq`, `--columns`, `--compact`/`--pretty`, `-v`/`-q`, JSON stderr logs.
  - `--fields`, `@file` and `-` for `--params`/`--json`, `--page-items`, resumable uploads with retry, `-o -` and `--decode-field`, `--wait` for long-running operations, `--no-quota-project`, `GWSR_API_BASE_URL`, `GWSR_RESTRICT_PATHS`.
  - `--dry-run` also previews `gwsr cache clear`, `gwsr dev generate-skills` and `gwsr dev man` without deleting or writing anything.
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

## 0.22.5

### Patch Changes

- 5d24ac2: Add cargo-audit CI workflow for automated dependency vulnerability scanning
- ecddf2e: Add cargo-deny configuration for license, advisory, and source auditing
- 503315b: Update installation instructions to prioritize GitHub Releases over npm
- 6ccbb42: fix: auto-install binary on run if missing

  pnpm skips postinstall when the package is already up to date.
  This ensures run.js will auto-trigger install.js if the
  binary is missing, fixing the 'gws binary not found' error.

- b307856: Migrated the internal AI skills registry (personas and recipes) from YAML to TOML. This allows us to drop the unmaintained serde_yaml dependency, improving the project's supply chain security posture.
- 158f93a: Verify SHA256 checksum of downloaded binary in npm postinstall script
- b422e5d: Pin cross-rs to v0.2.5 in release workflow to prevent unpinned git HEAD builds

## 0.22.4

### Patch Changes

- 86c08cf: Remove cargo-dist; use native Node.js fetch for npm binary installer

  Replaces the cargo-dist generated release pipeline and npm package with:

  - A custom GitHub Actions release workflow with matrix cross-compilation
  - A zero-dependency npm installer using native `fetch()` (Node 18+)
  - Removes axios, rimraf, detect-libc, console.table, and axios-proxy-builder dependencies from the published npm package

## 0.22.3

### Patch Changes

- 674d53a: Fix `Lint Skills` CI job by installing `uv` via `astral-sh/setup-uv` before running `uvx`
- c7c42f6: fix: register script service and resolve test path validation errors
- 80bd150: feat(auth): use strict OS keychain integration on macOS and Windows

  Closes #623. The CLI no longer writes a fallback `.encryption_key` text file on macOS and Windows when securely storing credentials. Instead, it strictly uses the native OS keychain (Keychain Access on macOS, Credential Manager on Windows). If an old `.encryption_key` file is found during a successful keychain login, it will be automatically deleted for security.
  Linux deployments continue to use a seamless file-based fallback by default to ensure maximum compatibility with headless continuous integration (CI) runners, Docker containers, and SSH environments without desktop DBUS services.

- ec7f56b: Sync generated skills with latest Google Discovery API specs

## 0.22.2

### Patch Changes

- a52d297: Improve proxy-aware OAuth flows and clean up review feedback for auth login.

## 0.22.1

### Patch Changes

- 6a45832: Sync generated skills with latest Google Discovery API specs

## 0.22.0

### Minor Changes

- 0850c48: Add `--draft` flag to Gmail `+send`, `+reply`, `+reply-all`, and `+forward` helpers to save messages as drafts instead of sending them immediately

## 0.21.2

### Patch Changes

- c4448b9: Add crates.io publishing to release workflow

  Publishes both `google-workspace` and `google-workspace-cli` to crates.io on each release. The library crate is published first (as a dependency), followed by the CLI crate.

## 0.21.1

### Patch Changes

- ea0849a: Fix version-sync script and bump CLI crate version to 0.21.0

  The `version-sync.sh` script was updating the root `Cargo.toml` which no longer has a `[package]` section after the workspace refactor. Updated to target `crates/google-workspace-cli/Cargo.toml`. Also syncs the CLI crate version to 0.21.0 to match `package.json`.

## 0.21.0

### Minor Changes

- 029e5de: Extract `google-workspace` library crate for programmatic Rust API access (closes #386)

  Introduces a Cargo workspace with a new `google-workspace` library crate (`crates/google-workspace/`)
  that exposes the core modules for use as a Rust dependency:

  - `discovery` — Discovery Document types and fetching
  - `error` — Structured `GwsError` type
  - `services` — Service registry and resolution
  - `validate` — Input validation and URL encoding
  - `client` — HTTP client with retry logic

  The `gws` binary crate re-exports all library types transparently — zero behavioral changes.

## 0.20.1

### Patch Changes

- b8fd3d9: fix(client): add 10s connect timeout to prevent hangs on initial connection
- 2bfcca9: Move version from top-level SKILL.md frontmatter to metadata and track CLI version
- 2ddb46e: test(gmail): add regression tests for RFC 2822 display name quoting
- 75a7121: Sync generated skills with latest Google Discovery API specs

## 0.20.0

### Minor Changes

- e782dd7: Forward original attachments by default and preserve inline images in HTML mode.

  `+forward` now includes the original message's attachments and inline images by default,
  matching Gmail web behavior. Use `--no-original-attachments` to opt out.
  `+reply`/`+reply-all` with `--html` preserve inline images in the quoted body via
  `multipart/related`. In plain-text mode, inline images are not included (matching Gmail web).

## 0.19.0

### Minor Changes

- a078945: Refactor all `gws auth` subcommands to use clap for argument parsing

  Replace manual argument parsing in `handle_auth_command`, `handle_login`, `resolve_scopes`, and `handle_export` with structured `clap::Command` definitions. Introduces `ScopeMode` enum for type-safe scope selection and adds proper `--help` support for all auth subcommands.

### Patch Changes

- 8a749c2: feat(helpers): add --dry-run support to events helper commands

  Add dry-run mode to `gws events +renew` and `gws events +subscribe` commands.
  When --dry-run is specified, the commands will print what actions would be
  taken without making any API calls. This allows agents to simulate requests
  and learn without reaching the server.

- d679401: Fix `mask_secret` panic on multi-byte UTF-8 secrets by using char-based indexing instead of byte-offset slicing
- d341de2: Handle --help/-h in `gws auth setup` before launching the setup wizard, preventing accidental project creation when users just want usage info
- f157208: fix: use block-style YAML sequences in generated SKILL.md frontmatter

  Replace flow sequences (`bins: ["gws"]`, `skills: [...]`) with block-style
  sequences (`bins:\n  - gws`) in all generated SKILL.md frontmatter templates.

  Flow sequences are valid YAML but rejected by `strictyaml`, which the
  Agent Skills reference implementation (`agentskills validate`) uses to parse
  frontmatter. This caused all 93 generated skills to fail validation.

  Fixes #521

- b4d5e26: Fix auth error propagation: properly propagate errors when token directory creation or permission setting fails, instead of silently ignoring them

## 0.18.1

### Patch Changes

- a87037b: Handle SIGTERM in `gws gmail +watch` and `gws events +subscribe` for clean container shutdown.

  Long-running pull loops now exit gracefully on SIGTERM (in addition to Ctrl+C),
  enabling clean shutdown under Kubernetes, Docker, and systemd.

## 0.18.0

### Minor Changes

- 908cf73: feat(gmail): auto-populate From header with display name from send-as settings

  Fetch the user's send-as identities to set the From header with a display name in all mail helpers (+send, +reply, +reply-all, +forward), matching Gmail web client behavior. Also enriches bare `--from` emails with their configured display name.

- 6e4daaf: Gmail helpers rollup: mail-builder migration, --attach flag (upload endpoint), +read helper

  - Migrate `+send`, `+reply`, `+reply-all`, and `+forward` to the `mail-builder` crate for RFC-compliant MIME construction
  - Add `--from` flag to `+send` for send-as alias support
  - Add `-a`/`--attach` flag to all mail helpers (`+send`, `+reply`, `+reply-all`, `+forward`) with `mime_guess2` auto-detection, 25MB size validation, and upload endpoint support (35MB API limit vs 5MB metadata-only)
  - Add `+read` helper to extract message body and headers (text, HTML, or JSON output)
  - Make `OriginalMessage.thread_id` optional (`Option<String>`) for draft compatibility
  - RFC 2822 display name quoting is handled natively by `mail-builder`
  - Introduce `UploadSource` enum in executor for type-safe upload strategies

### Patch Changes

- 1e90380: fix(gmail): remove dead `--attachment` arg from `+send`

  The `+send` subcommand defined a duplicate `"attachment"` arg alongside the
  `"attach"` arg already provided by `common_mail_args`. Since `parse_attachments`
  reads `"attach"`, the `--attachment` flag was silently ignored. Removed the
  dead duplicate.

- 908cf73: fix(gmail): handle reply-all to own message correctly

  Reply-all to a message you sent no longer errors with "No To recipient remains." The original To recipients are now used as reply targets, matching Gmail web client behavior.

- 2e909ae: Consolidate terminal sanitization, coloring, and output helpers into a new `output.rs` module. Fixes raw ANSI escape codes in `watch.rs` that bypassed `NO_COLOR` and TTY detection, upgrades `sanitize_for_terminal` to also strip dangerous Unicode characters (bidi overrides, zero-width spaces, directional isolates), and sanitizes previously raw API error body and user query outputs.

## 0.17.0

### Minor Changes

- 1b0a21f: feat: support google meet video conferencing in calendar +insert

### Patch Changes

- 811fe7b: Fix critical security vulnerability (TOCTOU/Symlink race) in atomic file writes.

  The atomic_write and atomic_write_async utilities now use:

  - Randomized temporary filenames to prevent predictability.
  - O_EXCL creation flags to prevent following pre-existing symlinks.
  - Strict 0600 permissions from the moment of file creation on Unix systems.
  - Redundant post-write permission calls have been removed to close race windows.

- b241a5b: fix(security): cap Retry-After sleep, sanitize upload mimeType, and validate --upload/--output paths
- 6f92e5b: Stderr/output hygiene rollup: route diagnostics to stderr, add colored error labels, propagate auth errors.

  - **triage.rs**: "No messages found" sent to stderr so stdout stays valid JSON for pipes
  - **modelarmor.rs**: response body printed only on success; error message now includes body for diagnostics
  - **error.rs**: colored `error[variant]:` labels on stderr (respects `NO_COLOR` env var), `hint:` prefix for accessNotConfigured guidance
  - **calendar, chat, docs, drive, script, sheets**: auth failures now propagate as `GwsError::Auth` instead of silently proceeding unauthenticated (dry-run still works without auth)

- 398e80c: Sync generated skills with latest Google Discovery API specs
- 8458104: Extend input validation to reject dangerous Unicode characters (zero-width chars, bidi overrides, Unicode line/paragraph separators) that were not caught by the previous ASCII-range check

## 0.16.0

### Minor Changes

- 47afe5f: Use Google account timezone instead of machine-local time for day-boundary calculations in calendar and workflow helpers. Adds `--timezone` flag to `+agenda` for explicit override. Timezone is fetched from Calendar Settings API and cached for 24 hours.

### Patch Changes

- c61b9cb: fix(gmail): RFC 2047 encode non-ASCII display names in To/From/Cc/Bcc headers

  Fixes mojibake when sending emails to recipients with non-ASCII display names (e.g. Japanese, Spanish accented characters). The new `encode_address_header()` function parses mailbox lists, encodes only the display-name portion via RFC 2047 Base64, and leaves email addresses untouched.

## 0.15.0

### Minor Changes

- 6f3e090: Add opt-in structured HTTP request logging via `tracing`

  New environment variables:

  - `GOOGLE_WORKSPACE_CLI_LOG`: stderr log filter (e.g., `gws=debug`)
  - `GOOGLE_WORKSPACE_CLI_LOG_FILE`: directory for JSON log files with daily rotation

  Logging is completely silent by default (zero overhead). Only PII-free metadata is logged: API method ID, HTTP method, status code, latency, and content-type.

## 0.14.0

### Minor Changes

- dc561e0: Add `--upload-content-type` flag and smart MIME inference for multipart uploads

  Previously, multipart uploads used the metadata `mimeType` field for both the Drive
  metadata and the media part's `Content-Type` header. This made it impossible to upload
  a file in one format (e.g. Markdown) and have Drive convert it to another (e.g. Google Docs),
  because the media `Content-Type` and the target `mimeType` must differ for import conversions.

  The new `--upload-content-type` flag allows setting the media `Content-Type` explicitly.
  When omitted, the media type is now inferred from the file extension before falling back
  to the metadata `mimeType`. This matches Google Drive's model where metadata `mimeType`
  is the _target_ type (what the file should become) while the media `Content-Type` is the
  _source_ type (what the bytes are).

  This means import conversions now work automatically:

  ```bash
  # Extension inference detects text/markdown → conversion just works
  gws drive files create \
    --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
    --upload notes.md

  # Explicit flag still available as an override
  gws drive files create \
    --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
    --upload notes.md \
    --upload-content-type text/markdown
  ```

### Patch Changes

- 945ac91: Stream multipart uploads to avoid OOM on large files. File content is now streamed in chunks via `ReaderStream` instead of being read entirely into memory, reducing memory usage from O(file_size) to O(64 KB).

## 0.13.3

### Patch Changes

- 8ef27a2: fix(calendar): use local timezone for agenda day boundaries instead of UTC
- 4d7b420: Fix `+append --json-values` flattening multi-row arrays into a single row by preserving the `Vec<Vec<String>>` row structure through to the API request body
- bb94016: fix(security): validate space name in chat +send to prevent path traversal
- 4b827cd: chore: fix maintainer email typo in flake.nix and harden coverage.sh
- 44767ed: Map People service to `contacts` and `directory` scope prefixes so `gws auth login -s people` includes the required OAuth scopes
- 8fce003: fix(docs): correct flag names in recipes (--spreadsheet-id, --attendees, --duration)
- 21b1840: Expose `repeated: true` in `gws schema` output and expand JSON arrays into repeated query parameters for `repeated` fields
- 1346d47: Sync generated skills with latest Google Discovery API specs
- 957b999: test(gmail): add unit tests for +triage argument parsing and format selection

## 0.13.2

### Patch Changes

- 3dcf818: Refresh OAuth access tokens for long-running Gmail watch and Workspace Events subscribe helpers before each Pub/Sub and Gmail request.
- 86ea6de: Validate `--subscription` resource name in `gmail +watch` and deduplicate `PUBSUB_API_BASE` constant.

## 0.13.1

### Patch Changes

- 510024f: Centralize token cache filenames as constants and support ServiceAccount credentials at the default plaintext path
- 510024f: Auto-recover from stale encrypted credentials after upgrade: remove undecryptable `credentials.enc` and fall through to other credential sources (plaintext, ADC) instead of hard-erroring. Also sync encryption key file backup when keyring has key but file is missing.
- e104106: Add shell tips section to gws-shared skill warning about zsh `!` history expansion, and replace single quotes with double quotes around sheet ranges containing `!` in recipes and skill examples

## 0.13.0

### Minor Changes

- 9d937af: Add `--html` flag to `+send`, `+reply`, `+reply-all`, and `+forward` for HTML email composition.

### Patch Changes

- 2df32ee: Document helper commands (`+` prefix) in README

  Adds a "Helper Commands" section to the Advanced Usage chapter explaining
  the `+` prefix convention, listing all 24 helper commands across 10 services
  with descriptions and usage examples.

## 0.12.0

### Minor Changes

- 247e27a: Add structured exit codes for scriptable error handling

  `gws` now exits with a type-specific code instead of always using `1`:

  | Code | Meaning                                                         |
  | ---- | --------------------------------------------------------------- |
  | `0`  | Success                                                         |
  | `1`  | API error — Google returned a 4xx/5xx response                  |
  | `2`  | Auth error — credentials missing, expired, or invalid           |
  | `3`  | Validation error — bad arguments, unknown service, invalid flag |
  | `4`  | Discovery error — could not fetch the API schema document       |
  | `5`  | Internal error — unexpected failure                             |

  Exit codes are documented in `gws --help` and in the README.

### Patch Changes

- 087066f: Fix `gws auth login` encrypted credential persistence by enabling native keyring backends for the `keyring` crate on supported desktop platforms instead of silently falling back to the in-memory mock store.

## 0.11.1

### Patch Changes

- adbca87: Fix `--format csv` for array-of-arrays responses (e.g. Sheets values API)

## 0.11.0

### Minor Changes

- 4d4b09f: Add `--cc` and `--bcc` flags to `+send`, `--to` and `--bcc` to `+reply` and `+reply-all`, and `--bcc` to `+forward`.

## 0.10.0

### Minor Changes

- 8d89325: Add `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND` env var for explicit keyring backend selection (`keyring` or `file`). Fixes credential key loss in Docker/keyring-less environments by never deleting `.encryption_key` and always persisting it as a fallback.

### Patch Changes

- 06aa698: fix(auth): dynamically fetch scopes from Discovery docs when `-s` specifies services not in static scope lists
- 06aa698: fix(auth): format extract_scopes_from_doc and deduplicate dynamic scopes
- 5e7d120: Bring `+forward` behavior in line with Gmail's web UI: keep the forward in the sender's original thread, add a blank line between the forwarded message metadata and body, and remove the spurious closing delimiter.
- 2782cf1: Fix gmail +triage 403 error by using gmail.readonly scope instead of gmail.modify to avoid conflict with gmail.metadata scope that does not support the q parameter

## 0.9.1

### Patch Changes

- 5872dbe: Stop persisting encryption key to `.encryption_key` file when OS keyring is available. Existing file-based keys are migrated into the keyring and the file is removed on next CLI invocation.

## 0.9.0

### Minor Changes

- 7d15365: feat(gmail): add +reply, +reply-all, and +forward helpers

  Adds three new Gmail helper commands:

  - `+reply` -- reply to a message with automatic threading
  - `+reply-all` -- reply to all recipients with --remove/--cc support
  - `+forward` -- forward a message to new recipients

### Patch Changes

- 08716f8: Fix garbled non-ASCII email subjects in `gmail +send` by RFC 2047 encoding the Subject header and adding MIME-Version/Content-Type headers.
- f083eb9: Improve `gws auth setup` project creation failures in step 3:
  - Detect Google Cloud Terms of Service precondition failures and show actionable guidance (`gcloud auth list`, account verification, Console ToS URL).
  - Detect invalid project ID format / already-in-use errors and show clearer guidance.
  - In interactive setup, keep the wizard open and re-prompt for a new project ID instead of exiting immediately on create failures.
- 789e7f1: Switch reqwest TLS from bundled Mozilla roots to native OS certificate store

  This allows the CLI to trust custom or corporate CA certificates installed
  in the system trust store, fixing TLS errors in enterprise environments.

## 0.8.1

### Patch Changes

- 4d41e52: Prioritize local project configuration and `GOOGLE_WORKSPACE_PROJECT_ID` over global Application Default Credentials (ADC) for quota attribution. This fixes 403 errors when the Drive API is disabled in a global gcloud project but enabled in the project configured for gws.

## 0.8.0

### Minor Changes

- dd3fc90: Remove `mcp` command

## 0.7.0

### Minor Changes

- e1505af: Remove multi-account, domain-wide delegation, and impersonation support. Removes `gws auth list`, `gws auth default`, `--account` flag, `GOOGLE_WORKSPACE_CLI_ACCOUNT` and `GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER` env vars.

### Patch Changes

- 54b3b31: Move x-goog-user-project header from default client headers to API request builder, fixing Discovery Document fetches failing with 403 when the quota project lacks certain APIs enabled

## 0.6.3

### Patch Changes

- 322529d: Document all environment variables and enable GOOGLE_WORKSPACE_CLI_CONFIG_DIR in release builds
- 2173a92: Send x-goog-user-project header when using ADC with a quota_project_id
- 1f47420: fix: extract CLA label job into dedicated workflow to prevent feedback loop

  The Automation workflow's `check_run: [completed]` trigger caused a feedback
  loop — every workflow completion fired a check_run event, re-triggering
  Automation, which produced another check_run event, and so on. Moving the
  CLA label job to its own `cla.yml` workflow eliminates the trigger from
  Automation entirely.

- 132c3b1: fix: warn on credential file permission failures instead of ignoring

  Replaced silent `let _ =` on `set_permissions` calls in `save_encrypted`
  with `eprintln!` warnings so users are aware if their credential files
  end up with insecure permissions. Also log keyring access failures
  instead of silently falling through to file storage.

- a2cc523: Add `x86_64-unknown-linux-musl` build target for Linux musl/static binary support
- c86b964: Fix multi-account selection: MCP server now respects `GOOGLE_WORKSPACE_CLI_ACCOUNT` env var (#221), and `--account` flag before service name no longer causes parse errors (#181)
- ff53538: Fix scope selection to use first (broadest) scope instead of all method scopes, preventing gmail.metadata restrictions from blocking query parameters
- c80eb52: Replace strip_suffix(".readonly").unwrap() with unwrap_or fallback

  Two call sites used `.strip_suffix(".readonly").unwrap()` which would
  panic if a scope URL marked as `is_readonly` didn't actually end with
  ".readonly". While the current data makes this unlikely, using
  `unwrap_or` is a defensive improvement that prevents potential panics
  from inconsistent discovery data.

- 9a780d7: Log token cache decryption/parse errors instead of silently swallowing

  Previously, `load_from_disk` used four nested `if let Ok` blocks that
  silently returned an empty map on any failure. When the encryption key
  changed or the cache was corrupted, tokens silently stopped loading and
  users were forced to re-authenticate with no explanation.

  Now logs specific warnings to stderr for decryption failures, invalid
  UTF-8, and JSON parse errors, with a hint to re-authenticate.

- 6daf90d: Fix MCP tool schemas to conditionally include `body`, `upload`, and `page_all` properties only when the underlying Discovery Document method supports them. `body` is included only when a request body is defined, `upload` only when `supportsMediaUpload` is true, and `page_all` only when the method has a `pageToken` parameter. Also drops empty `body: {}` objects that LLMs commonly send on GET methods, preventing 400 errors from Google APIs.

## 0.6.2

### Patch Changes

- 28fa25a: Clean up nits from PR #175 auth fix

  - Update stale docstring on `resolve_account` to match new fallthrough behavior
  - Add breadcrumb comment on string-based error matching in `main.rs`
  - Move identity scope injection before authenticator build for readability

## 0.6.1

### Patch Changes

- 88cb65c: chore: add automation workflow for auto-fmt, CLA labeling, and file-based PR triage
- a926e3f: Fix auth failures when accounts.json registry is missing

  Three related bugs caused all API calls to fail with "Access denied. No credentials provided" even after a successful `gws auth login`:

  1. `resolve_account()` rejected valid `credentials.enc` as "legacy" when `accounts.json` was absent, instead of using them.
  2. `main.rs` silently swallowed all auth errors, masking real failures behind a generic message.
  3. `auth login` didn't include `openid`/`email` scopes, so `fetch_userinfo_email()` couldn't identify the user, causing credentials to be saved without an `accounts.json` entry.

- cb1f988: Add Content-Length: 0 header for POST/PUT/PATCH requests with no body to fix HTTP 411 errors
- 3d59b2e: fix: isolate flaky auth tests from host ADC credentials

## 0.6.0

### Minor Changes

- b38b760: Add Application Default Credentials (ADC) support.

  `gws` now discovers ADC as a fourth credential source, after the encrypted
  and plaintext credential files. The lookup order is:

  1. `GOOGLE_WORKSPACE_CLI_TOKEN` env var (raw access token, highest priority)
  2. `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var
  3. Encrypted credentials (`~/.config/gws/credentials.enc`)
  4. Plaintext credentials (`~/.config/gws/credentials.json`)
  5. **ADC** — `GOOGLE_APPLICATION_CREDENTIALS` env var (hard error if file missing), then
     `~/.config/gcloud/application_default_credentials.json` (silent if absent)

  This means `gcloud auth application-default login --client-id-file=client_secret.json`
  is now a fully supported auth flow — no need to run `gws auth login` separately.
  Both `authorized_user` and `service_account` ADC formats are supported.

## 0.5.0

### Minor Changes

- 9cf6e0e: Add `--tool-mode compact|full` flag to `gws mcp`. Compact mode exposes one tool per service plus a `gws_discover` meta-tool, reducing context window usage from 200-400 tools to ~26.

### Patch Changes

- 0a16d0b: Add `-s`/`--services` flag to `gws auth login` to filter the scope picker
  by service name (e.g. `-s drive,gmail,sheets`). Also expands the workspace
  admin scope blocklist to include `chat.admin.*` and `classroom.*` patterns.
- 5205467: fix(setup): drain stale keypresses between TUI screen transitions

## 0.4.4

### Patch Changes

- e1e08eb: Fix highlight color on light terminal themes by using reverse video instead of a dark-gray background

## 0.4.3

### Patch Changes

- fc6bc95: Exclude Workspace-admin-only scopes from the "Recommended" scope preset.

  Scopes that require Google Workspace domain-admin access (`apps.*`,
  `cloud-identity.*`, `ediscovery`, `directory.readonly`, `groups`) now return
  `400 invalid_scope` when used by personal `@gmail.com` accounts. These scopes
  are no longer included in the "Recommended" template, preventing login failures
  for non-Workspace users.

  Workspace admins can still select these scopes manually via the "Full Access"
  template or by picking them individually in the scope picker.

  Adds a new `is_workspace_admin_scope()` helper (mirroring the existing
  `is_app_only_scope()`) that centralises this detection logic.

- 2aa6084: docs: Comprehensive README overhaul addressing user feedback.

  Added a Prerequisites section prior to the Quick Start to highlight the optional `gcloud` dependency.
  Expanded the Authentication section with a decision matrix to help users choose the correct authentication path.
  Added prominent warnings about OAuth "testing mode" limitations (the 25-scope cap) and the strict requirement to explicitly add the authorizing account as a "Test user" (#130).
  Added a dedicated Troubleshooting section detailing fixes for common OAuth consent errors, "Access blocked" issues, and `redirect_uri_mismatch` failures.
  Included shell escaping examples for Google Sheets A1 notation (`!`).
  Clarified the `npm` installation rationale and added explicit links to pre-built native binaries on GitHub Releases.

## 0.4.2

### Patch Changes

- d3e90e4: fix: use ~/.config/gws on all platforms for consistent config path

  Previously used `dirs::config_dir()` which resolves to different paths per OS
  (e.g. ~/Library/Application Support/gws on macOS, %APPDATA%\gws on Windows),
  contradicting the documented ~/.config/gws/ path. Now uses ~/.config/gws/
  everywhere with a fallback to the legacy OS-specific path for existing installs.

## 0.4.1

### Patch Changes

- dbda001: Add "Enter project ID manually" option to project picker in `gws auth setup`.

  Users with large numbers of GCP projects often hit the 10-second listing timeout.
  The picker now includes a "⌨ Enter project ID manually" item so users can type a
  known project ID directly without waiting for `gcloud projects list` to complete.

## 0.4.0

### Minor Changes

- 87e4bb1: Add Linux ARM64 build targets (aarch64-unknown-linux-gnu and aarch64-unknown-linux-musl) to cargo-dist, enabling prebuilt binaries for ARM64 Linux users via npm, the shell installer, and GitHub Releases.
- d1825f9: ### Multi-Account Support

  Add support for managing multiple Google accounts with per-account credential storage.

  **New features:**

  - `--account EMAIL` global flag available on every command
  - `GOOGLE_WORKSPACE_CLI_ACCOUNT` environment variable as fallback
  - `gws auth login --account EMAIL` — associates credentials with a specific account
  - `gws auth list` — lists all registered accounts
  - `gws auth default EMAIL` — sets the default account
  - `gws auth logout --account EMAIL` — removes a specific account
  - `login_hint` in OAuth URL for automatic account pre-selection in browser
  - Email validation via Google userinfo endpoint after OAuth flow

  **Breaking change:** Existing users must run `gws auth login` again after upgrading. The credential storage format has changed from a single `credentials.enc` to per-account files (`credentials.<b64-email>.enc`) with an `accounts.json` registry.

### Patch Changes

- a6994ad: Filter out `apps.alerts` scopes from user OAuth login flow since they require service account with domain-wide delegation
- 1ad4f34: fix: replace unwrap() calls with proper error handling in MCP server

  Replaced four `unwrap()` calls in `mcp_server.rs` that could panic the MCP
  server process with graceful error handling. Also added a warning log when
  authentication silently falls back to unauthenticated mode.

- a1be14f: fix: drain stdout pipe to prevent project listing timeout during auth setup

  Fixed `gws auth setup` timing out at step 3 (GCP project selection) for users
  with many projects. The `gcloud projects list` stdout pipe was only read after
  the child process exited, causing a deadlock when output exceeded the OS pipe
  buffer (~64 KB). Stdout is now drained in a background thread to prevent the
  pipe from filling up.

- 364542b: fix: reject DEL character (0x7F) in input validation

  The `reject_control_chars` helper rejected bytes 0x00–0x1F but allowed
  the DEL character (0x7F), which is also an ASCII control character. This
  could allow malformed input from LLM agents to bypass validation.

- 75cec1b: Fix URL template expansion so media upload endpoints substitute path parameters and avoid iterative replacement side effects.
- ed409e3: Harden URL and path construction across helper modules (gmail/watch, modelarmor, discovery)
- 263a8e5: fix: use gcloud.cmd on Windows and show platform-correct config paths

  On Windows, gcloud is installed as `gcloud.cmd` which Rust's `Command`
  cannot find without the extension. Also replaced hardcoded `~/.config/gws/`
  in error messages with the actual platform-resolved path.

## 0.3.5

### Patch Changes

- 4bca693: fix: credential masking panic and silent token write errors

  Fixed `gws auth export` masking which panicked on short strings and showed
  the entire secret instead of masking it. Also fixed silent token cache write
  failures in `save_to_disk` that returned `Ok(())` even when the write failed.

- f84ce37: Fix URL template path expansion to safely encode path parameters, including
  Sheets `range` values with Unicode and reserved characters. `{var}` expansions
  now encode as a path segment, `{+var}` preserves slashes while encoding each
  segment, and invalid path parameter/template mismatches fail fast.
- eb0347a: fix: correct author email typo in package.json
- 70d0cdd: Fix Slides presentations.get failure caused by flatPath placeholder mismatch

  When a Discovery Document's `flatPath` uses placeholder names that don't match
  the method's parameter names (e.g., `{presentationsId}` vs `presentationId`),
  `build_url` now falls back to the `path` field which uses RFC 6570 operators
  that resolve correctly.

  Fixes #118

- 37ab483: Add flake.nix for nix & NixOS installs
- 1991d53: Add prominent disclaimer that this is not an officially supported Google product to README, --help, and --version output

## 0.3.4

### Patch Changes

- 704928b: fix(setup): enable APIs individually and surface gcloud errors

  Previously `gws auth setup` used a single batch `gcloud services enable` call
  for all Workspace APIs. If any one API failed, the entire batch was marked as
  failed and stderr was silently discarded. APIs are now enabled individually and
  in parallel, with error messages surfaced to the user.

## 0.3.3

### Patch Changes

- 92e66a3: Add `gws version` as a bare subcommand alongside `gws --version` and `gws -V`

## 0.3.2

### Patch Changes

- 8fadbd6: Smarter truncation of method and resource descriptions from discovery docs. Descriptions now truncate at sentence boundaries when possible, fall back to word boundaries with an ellipsis, and strip markdown links to reclaim character budget. Fixes #64.

## 0.3.1

### Patch Changes

- b3669e0: Add hourly cron to generate-skills workflow to auto-sync skills with upstream Google Discovery API changes via PR
- e8d533e: Add workflow to publish OpenClaw skills to ClawHub
- 3b38c8d: Sync generated skills with latest Google Discovery API specs

## 0.3.0

### Minor Changes

- 670267f: feat: add `gws mcp` Model Context Protocol server

  Adds a new `gws mcp` subcommand that starts an MCP server over stdio,
  exposing Google Workspace APIs as structured tools to any MCP-compatible
  client (Claude Desktop, Gemini CLI, VS Code, etc.).

### Patch Changes

- 8c1042a: Fix x-goog-api-client header format to use `gl-rust/gws-<version>`
- 3de9762: Fix docs: `gws setup` → `gws auth setup` (fixes #56, #57)

## 0.2.2

### Patch Changes

- f281797: docs(auth): add manual Google Cloud OAuth client setup and browser-assisted login guidance

  Adds step-by-step guidance for creating a Desktop OAuth client in Google Cloud Console,
  where to place `client_secret.json`, and how humans/agents can complete browser consent
  (including unverified app and scope-selection prompts).

- ee2e216: Narrow default OAuth scopes to avoid `Error 403: restricted_client` on unverified apps and add a `--full` flag for broader access (fixes #25). Replace the cryptic non-interactive setup error with actionable step-by-step OAuth console instructions (fixes #24).
- de2787e: feat(error): detect disabled APIs and guide users to enable them

  When the Google API returns a 403 `accessNotConfigured` error (i.e., the
  required API has not been enabled for the GCP project), `gws` now:

  - Extracts the GCP Console enable URL from the error message body.
  - Prints the original error JSON to stdout (machine-readable, unchanged shape
    except for an optional new `enable_url` field added to the error object).
  - Prints a human-readable hint with the direct enable URL to stderr, along
    with instructions to retry after enabling.

  This prevents a dead-end experience where users see a raw 403 JSON blob
  with no guidance. The JSON output is backward-compatible; only an optional
  `enable_url` field is added when the URL is parseable from the message.

  Fixes #31

- 9935dde: ci: auto-generate and commit skills on PR branch pushes
- 4b868c7: docs: add community guidance to gws-shared skill and gws --help output

  Encourages agents and users to star the repository and directs bug reports
  and feature requests to GitHub Issues, with guidance to check for existing
  issues before opening new ones.

- 0603bce: fix: atomic credential file writes to prevent corruption on crash or Ctrl-C
- 666f9a8: fix(auth): support --help / -h flag on auth subcommand
- bcd2401: fix: flatten nested objects in table output and fix multi-byte char truncation panic
- ee35e4a: fix: warn to stderr when unknown --format value is provided
- e094b02: fix: YAML block scalar for strings with `#`/`:`, and repeated CSV/table headers with `--page-all`

  **Bug 1 — YAML output: `drive#file` rendered as block scalar**

  Strings containing `#` or `:` (e.g. `drive#file`, `https://…`) were
  incorrectly emitted as YAML block scalars (`|`), producing output like:

  ```yaml
  kind: |
    drive#file
  ```

  Block scalars add an implicit trailing newline which changes the string
  value and produces invalid-looking output. The fix restricts block
  scalar to strings that genuinely contain newlines; all other strings
  are double-quoted, which is safe for any character sequence.

  **Bug 2 — `--page-all` with `--format csv` / `--format table` repeats headers**

  When paginating with `--page-all`, each page printed its own header row,
  making the combined output unusable for downstream processing:

  ```
  id,kind,name          ← page 1 header
  1,drive#file,foo.txt
  id,kind,name          ← page 2 header (unexpected!)
  2,drive#file,bar.txt
  ```

  Column headers (and the table separator line) are now emitted only for
  the first page; continuation pages contain data rows only.

- 173d155: fix: add YAML document separators (---) when paginating with --page-all --format yaml
- 214fc18: ci: skip smoketest on fork pull requests

## 0.2.1

### Patch Changes

- 6ae7427: fix(auth): stabilize encrypted credential key fallback across sessions

  When the OS keyring returned `NoEntry`, the previous code could generate
  a fresh random key on each process invocation instead of reusing one.
  This caused `credentials.enc` written by `gws auth login` to be
  unreadable by subsequent commands.

  Changes:

  - Always prefer an existing `.encryption_key` file before generating a new key
  - When generating a new key, persist it to `.encryption_key` as a stable fallback
  - Best-effort write new keys into the keyring as well
  - Fix `OnceLock` race: return the already-cached key if `set` loses a race

  Fixes #27

## 0.2.0

### Minor Changes

- b0d0b95: Add workflow helpers, personas, and 50 consumer-focused recipes

  - Add `gws workflow` subcommand with 5 built-in helpers: `+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`
  - Add 10 agent personas (exec-assistant, project-manager, sales-ops, etc.) with curated skill sets
  - Add `docs/skills.md` skills index and `registry/recipes.yaml` with 50 multi-step recipes for Gmail, Drive, Docs, Calendar, and Sheets
  - Update README with skills index link and accurate skill count
  - Fix lefthook pre-commit to run fmt and clippy sequentially

### Patch Changes

- 90adcb4: fix: percent-encode path parameters to prevent path traversal
- e71ce29: Fix Gemini extension installation issue by removing redundant authentication settings and update the documentation.
- 90adcb4: fix: harden input validation for AI/LLM callers

  - Add `src/validate.rs` with `validate_safe_output_dir`, `validate_msg_format`, and `validate_safe_dir_path` helpers
  - Validate `--output-dir` against path traversal in `gmail +watch` and `events +subscribe`
  - Validate `--msg-format` against allowlist (full, metadata, minimal, raw) in `gmail +watch`
  - Validate `--dir` against path traversal in `script +push`
  - Add clap `value_parser` constraint for `--msg-format`
  - Document input validation patterns in `AGENTS.md`

- 90adcb4: Security: Harden validate_resource_name and fix Gmail watch path traversal
- 90adcb4: Replace manual `urlencoded()` with reqwest `.query()` builder for safer URL encoding
- c11d3c4: Added test coverage for `EncryptedTokenStorage::new` initialization.
- 7664357: Add test for missing error path in load_client_config
- 90adcb4: fix: add shared URL safety helpers for path params (`encode_path_segment`, `validate_resource_name`)
- 90adcb4: fix: warn on stderr when API calls fail silently

## 0.1.5

### Patch Changes

- d29f41e: Fix README typography and spacing

## 0.1.4

### Patch Changes

- adb2cfa: Fix OAuth login failing with "no refresh token" error by decrypting the token cache before parsing and supporting the HashMap token format used by EncryptedTokenStorage
- d990dcc: Improve README branding by making the hero banner full-width.

## 0.1.3

### Patch Changes

- c714f4b: Fix npm package name to publish as @googleworkspace/cli instead of gws

## 0.1.2

### Patch Changes

- 3cd4d52: Fix release pipeline to sync Cargo.toml version with changesets and create git tags for private packages

## 0.1.1

### Patch Changes

- a0ad089: Speed up CI builds with Swatinem/rust-cache, sccache, and build artifact reuse for smoketests
- 30d929b: Optimize demo GIF and improve README
