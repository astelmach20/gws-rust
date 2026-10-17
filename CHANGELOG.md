# gws-rust

> Entries from 0.22.5 down are the history of the upstream project,
> [googleworkspace/cli](https://github.com/googleworkspace/cli), from which gws-rust was forked.

## 0.24.0

### Minor Changes

- bcd8022: `gmail +read` JSON output now includes the Gmail API `id` and `threadId` (the same names `+search` and `+triage` use), so `.id` can be passed straight to `+reply`, `+reply-all`, `+forward` and other `--message-id` flags. Breaking: the RFC 5322 `Message-ID` header moves from `message_id` to `message_id_header`, so it can no longer be mistaken for the API ID, and `thread_id` is renamed to `threadId`.

### Patch Changes

- a0319b4: The `quota_project_id` from gcloud Application Default Credentials is now sent as `x-goog-user-project` only when ADC is the credential in use. With a `gwsr` profile, `GWSR_CREDENTIALS_FILE` or `GWSR_TOKEN`, and no `GWSR_PROJECT_ID` or client `project_id`, no quota project is sent instead of an unrelated gcloud project (which failed with a 403 naming that project).
- 4848212: `admin-reports +audit --application` now accepts every application the Reports API lists in its Discovery document, such as `contacts`, `takeout`, `voice`, `meet_hardware` and `admin_data_action`. The helper used a hard-coded list of 25 applications and refused the rest, even though the API supports them.
- 35e00db: `admin +audit` keeps the fractional seconds of `--since` and `--until` instead of truncating them, so events in the last sub-second of the range are no longer dropped.
- 4c63e5f: `admin-reports +audit --application gmail` now works without `--until`. The Reports API rejects Gmail activity queries that have no `endTime`, so the helper sends the current time when `--until` is omitted, and refuses a Gmail range longer than the API's 30-day limit with a validation error (exit 3) instead of a 400 from the API.
- 3a89016: `admin-reports +audit --since` no longer crashes (exit 101) on a value whose last character is not ASCII (such as `7é`) or on a look-back too large for the calendar (such as `99999999999999d`). These values are now rejected as validation errors (exit 3).
- bcd880b: `admin-reports +audit` checks its time range locally: a `--since` that is not before `--until`, or that is in the future, is a validation error (exit 3) before any request, instead of a 400 from the Reports API. A `--since` look-back (`24h`, `7d`, and the `1d` default) now counts back from `--until` when it is given, so `--until` alone with an older date queries the day before it rather than failing because the start was still a day before now.
- 2c9fe7b: Breaking: `admin +user-suspend` no longer has a `--reason` flag. The reason was sent as the user's `suspensionReason`, which the Directory API marks output-only and ignores, so the account never recorded it, even though the help said it did. Passing `--reason` is now a usage error instead of being silently dropped.
- 3fd60c3: `admin +user-create --org-unit` now refuses a value that does not start with `/` (such as `Sales`) with a validation error (exit 3) before reading a password or sending anything. The Directory API takes an organizational unit's full path from the root (`/Sales`, `/Sales/EMEA`); before, a bare unit name was sent as is and the API rejected it.
- 972db11: `calendar +agenda --dry-run` now shows the `maxResults=250` query parameter on its `calendarList` request, matching the request the command actually sends.
- db4fffd: `calendar +agenda` no longer reports every per-calendar failure as an internal error (exit 5). A calendar that cannot be read now keeps its documented exit code (1 for API errors, 2 for auth, 6 for 429/5xx rate limits, 10 for network errors), with the calendar name still in the message.
- a8599fc: `calendar +agenda --calendar-id ID` now finds calendars that are hidden in your calendar list. Before, it failed with "--calendar-id not found in your calendar list" because the calendar list omits hidden entries unless asked for them. Without `--calendar-id`, hidden calendars are still left out of the agenda.
- bfaf9b6: `calendar +agenda --calendar-id` now fails with a validation error naming every requested calendar ID that is not in your calendar list. Previously, when at least one requested ID matched, the others were silently dropped and the agenda looked complete.
- 63d866f: An API call whose access token is still rejected with HTTP 401 after the automatic refresh now fails as an auth error: exit code `2` with `"reason":"authError"`, as the exit-code table documents for expired or invalid credentials. It used to exit `1` ("API error, don't retry unchanged"), which pointed agents at the request instead of at `gwsr auth login`. The server's message and reason are kept in the error message.
- 8d48891: An API error's `code` and exit code now follow the HTTP status, not the `error.code` in the response body. The retry policy already follows the HTTP status. When the two disagree, for example HTTP 503 with a body code of 400, a request was retried as transient but then exited `1` (permanent). An HTTP 400 with a body code of 503 was never retried but exited `6` (retryable). Both now agree with the HTTP status.
- 92deece: API errors in Google's current error format now take their `reason` from the `google.rpc.ErrorInfo` entry in `details` instead of the coarse `status`. A 403 `RATE_LIMIT_EXCEEDED` now exits with the retryable code `6` (it was `1`, even though the request had already been retried as rate-limited), and a `SERVICE_DISABLED` error now includes `enable_url` and the "API not enabled" hint.
- fc32d34: When a generated method that changes something (a non-GET request) succeeds but its result cannot be saved with `-o` (for example because of a bad `--decode-field`), the error now says that the change was made and names the resource. The same applies when `--jq` or `--format` fails on the saved-file summary, as already happens for printed results.
- ba73cbb: `admin-reports +audit --application gmail --since 30d` no longer fails with "Gmail audit queries cover at most 30 days". The default end time (now) is taken in whole seconds, like the look-back it counts from, so a 30-day look-back spans exactly 30 days instead of 30 days and a fraction of a second.
- d9588f5: Shell completion now completes `gwsr auth` subcommands and their flags (`gwsr auth lo<TAB>` offers `login`/`logout`, `gwsr auth status --<TAB>` offers `--offline`), and `gwsr dev man` writes a page for each `auth` subcommand (`gwsr-auth-login.1`, `gwsr-auth-status.1`, ...). Previously both saw only `auth`'s passthrough argument, so nothing under `auth` was completed or documented.
- 7741443: `gwsr auth export --unmasked --output` refuses a path that names a directory (`out/`, `out/.` or an existing directory) with a validation error (exit 3), before any credentials are read. `--output out/` used to drop the trailing separator and write the unmasked secrets to a new file named `out`.
- e6ef0fe: `GWSR_TOKEN_FILE`, `GWSR_CREDENTIALS_FILE` and Application Default Credentials files that start with a UTF-8 byte order mark are now read. Windows PowerShell 5.1 (`Set-Content -Encoding UTF8`, `Out-File`) and older Notepad write one. Before, the token was refused as containing non-ASCII characters and the JSON was refused as invalid.
- 4d87088: `--dry-run` given before `auth` is no longer silently ignored. `gwsr --dry-run auth logout` used to delete the profile's credentials and revoke the refresh token for real (and `login`, `use` and `export` ran for real too). The auth subcommands without a dry-run mode now refuse it with a validation error (exit 3) and do nothing; `gwsr --dry-run auth setup` passes the flag on to setup's own dry run.
- 000b005: `gwsr auth` help (`gwsr auth`, `gwsr auth <command> --help`, `gwsr help auth ...`) piped into a reader that closes early, such as `| head`, now exits 0 quietly like every other help page, instead of printing a `failed to print help: Broken pipe` validation error and exiting 3.
- 332e9ae: Shell completion and `gwsr dev man` now cover `gwsr auth setup`'s own flags (`--project`, `--login`, `--dry-run`, `--non-interactive`) instead of a generic `ARGS` placeholder.
- e53294d: `gwsr auth status` no longer reports `"authenticated": false, "verified": true` when Google's token endpoint cannot be reached. Without a response nothing was verified, so the report now matches `--offline`: `"authenticated": true, "verified": false`, with the network failure in `verification_error`. A credential that Google actually rejects is still reported with `token_error`.
- 2ae404c: `-o/--output` waits for a long-running operation on its own only for methods that return one, so saving a resource that merely looks like an operation (e.g. a People contact group, with `name` and `metadata`) no longer fails with "--wait: the people API has no operations.get method to poll".
- ab4b106: `gwsr batch classroom` now sends at most 50 calls per batch request, the Classroom API's documented limit. With 51 to 100 calls it used to send them in one request, which Classroom rejects as a whole. Other APIs still use groups of up to 100, and results stay in input order.
- 0bfc2a4: `gwsr batch` sends to the batch endpoint named by the API's Discovery `batchPath` (e.g. `https://sheets.googleapis.com/batch`) instead of a guessed `batch/<api>/<version>`, and refuses APIs that do not support batching.
- 6c3f271: `gwsr batch` rejects call ids containing control characters (ESC, NUL, tab, DEL) or invisible/bidi Unicode, which previously went raw into the MIME `Content-ID` header and the terminal confirmation prompt.
- b7e5c19: `gwsr batch` now accepts a Discovery method id whose prefix is not the API name, such as `directory.users.get` for `gwsr batch admin`, instead of reporting an unknown method.
- 13d4c9c: `gwsr batch` now splits each response part's headers from its body at the first blank line, whether it ends in `\r\n` or `\n`. Before, a part with `\n` headers whose JSON body contained `\r\n\r\n` was split inside the body and failed with "malformed batch part status line".
- bf7015f: `gwsr batch` now applies `--jq`, `--format` and `--columns` to its result lines, as it does for a `--page-all` stream. Before, the results were always printed as raw NDJSON and these options were silently ignored, although `--dry-run` did apply `--jq`. For example, `gwsr batch gmail --jq .body.snippet` printed the whole result objects instead of the snippets.
- 6934c4b: When `gwsr batch` stops because a result line cannot be printed (a `--jq` or `--format` error, or a failed write to stdout), the error now names the calls that were already applied but whose results were not printed, and the calls that were never sent. Before, it reported only the `--jq` error (exit 3), as if nothing had been sent, so re-running the batch could repeat writes that had already been applied.
- de735bf: `gwsr batch` now rejects a call whose `params` set `alt` to anything other than `json` (for example `alt=media` to download a Drive file, or `alt=proto`). The error names the input line. Batch results are decoded as JSON text, and Google does not serve media downloads in a batch, so such a call used to be sent and its result printed as a mangled string. Download media with a separate command instead.
- d3e20c5: `gwsr batch` now retries a batch request after a retryable failure (503, timeout, connection reset) when its POST/PATCH calls carry a `requestId`, as a single call already does. A batch with a POST or PATCH call that has no `requestId` is still never re-sent, because the server may already have applied it.
- bb0912d: `gwsr batch` finds a method named by its resource path when the API has a resource of its own name: `{"method": "tasks.list"}` in the `tasks` API (likewise `forms.get`, `people.get`) no longer fails as an unknown method.
- 7ce0ab5: `gwsr batch` now prints results in input order even when Google returns the parts of a batch response in a different order, matching each part to its call by Content-ID. Duplicate call ids are rejected, including a generated id (a line's position) that equals an explicit id on another line. Previously both results were labeled with the same id.
- 26d159a: `gwsr batch` now exits `6` (retry later) when every failed call failed with a retryable error (HTTP 408, 429, 500, 502, 503, 504, or a 403 rate limit such as `userRateLimitExceeded`), the same code each call would get on its own. Before, any per-call failure exited `1` (permanent), so a script could not tell that running the batch again would help. If any call failed permanently, the exit code is still `1`. The reason stays `batchPartialFailure`.
- ad1d8bb: `gwsr batch` now honors `--sanitize` / `GWSR_SANITIZE_TEMPLATE`: every result body is screened by Model Armor before it is printed, and block mode fails closed exactly as for a single call. Previously batch ignored the setting and printed API content unscreened, even in block mode.
- abef92b: When `gwsr batch` stops early (a later batch request fails, or `--sanitize` withholds a result), the error now names the calls that got no result and the calls that were never sent, and `--sanitize` no longer drops the other already-applied results in the same group.
- e80d61f: `gwsr batch` now sends `supportsAllDrives=true` by default on methods that have it, like the generated commands, so batch calls on shared-drive files no longer fail with 404.
- b4fc479: Tests: pass optional credentials to `run_batch` in the batchPath tests (fixes the test build on main).
- f05ffda: `gwsr batch` now rejects input lines with keys other than `id`, `method`, `params` and `json` (for example a misspelled `body` or `parms`), `json` on a method that takes no request body, and `upload` on any method. Previously those keys were silently dropped and the call was sent without them.
- 62e71f6: `--json` request-body validation now range-checks `float` fields. A value beyond the 32-bit float range (`1e39`, or the string `"1e39"`) used to pass validation and be sent, and the API's JSON parser then rejected it. `double` fields still take any finite number.
- a9f14bc: `--json` and `gwsr batch` bodies accept a JSON integer for a 64-bit integer field (Discovery `"type": "string"` with `"format": "int64"` or `"uint64"`), as the APIs do. Before, `{"size": 5}` failed validation with "Expected type 'string', found integer". Fractions and values outside the format's range are still rejected.
- e8fbe68: `--json` request-body validation now checks the string value of an `int64` or `uint64` field: it must be a decimal integer in that format's range, as a JSON number for the field and the string form of an `int32` field already had to be. `"abc"`, `"1.5"` or `"-1"` for a `uint64` used to pass validation and be sent, and the API then rejected the request. A type error now also names the field's format (`Expected type 'string' with format 'int64', found string`).
- f3751c3: `--json` request-body validation now accepts the string forms the proto3 JSON mapping allows: integers as strings (`"5"`, range-checked for `int32` and `uint32`) and floating-point numbers as strings (`"1.5"`, `"NaN"`, `"Infinity"`, `"-Infinity"`). Google APIs accept these bodies, but `gwsr` used to refuse them before sending.
- 9b6f7c6: `--json` bodies are no longer rejected for keys inside map-typed schemas such as Admin Directory `customSchemas` (`UserCustomProperties`); only the values are type-checked.
- 9c22b8d: `--json` bodies may set a field or map entry to `null` to clear it (for example, removing a Drive `appProperties` key on `files update`). Before this fix, schema validation rejected these bodies with "Expected type ..., found null".
- 3337d76: `gwsr cache clear` no longer fails when another `gwsr` process uses the Discovery cache at the same time. A file that disappears before it is deleted (a concurrent write finished, or another clear removed it) is skipped instead of reported as an error, and documents cached while the clear runs are removed too instead of failing with "Directory not empty". Other failures, such as permission errors, are still reported. The reported count is the number of documents this clear actually removed.
- 1727748: Discovery documents and account time zones are no longer read from a cache directory or file that belongs to another user. Group- or world-writable cache files were already refused, but a cache that another user created with an ordinary mode such as `0755`/`0644` was trusted. That can happen under a shared directory such as `GWSR_CACHE_DIR=/tmp/gwsr`. A planted Discovery document could remap a command to a different HTTP method and path, which then ran with your credentials. Such entries are now ignored and fetched again.
- 5012768: `calendar +freebusy` with date-only `--start`/`--end` no longer fails in time zones whose clocks spring forward across midnight (for example America/Santiago on 2026-09-06): the day starts at its first existing local time. `calendar +agenda` also sorts an all-day event on such a day before that day's timed events instead of last.
- 33ad258: `calendar +rsvp` and `calendar +update` no longer risk removing guests from an event whose guest list is hidden from you (`attendeesOmitted`). In that case Google returns only your own entry, and writing that list back would uninvite everyone else. `+rsvp` now sends only your entry with `attendeesOmitted: true`, which tells the API to update just your response. `+update --add-attendee/--remove-attendee` now refuses such an event and sends nothing.
- 6cb0c23: `calendar +insert`, `+freebusy` and `+update` now reject an all-day date at the end of the representable range (e.g. `--start +262142-12-31`) with a validation error instead of panicking.
- 0530ab6: `gwsr calendar +agenda --today` / `--tomorrow` and `gwsr workflow +standup-report` now cover the whole local calendar day, from local midnight to the next local midnight. On daylight-saving transition days the window used to be a fixed 24 hours, so it dropped the last hour of a 25-hour fall-back day and spilled an hour into the next day after a 23-hour spring-forward day. Days whose midnight does not exist (zones that spring forward at midnight) now start at the first valid local time instead of failing.
- 0944e33: calendar `+insert --duration` and `+update` (keeping an event's length when only `--start` changes) now add elapsed time across a daylight-saving change. A two-hour event starting at 00:30 on a fall-back night used to end at 02:30 wall-clock time (three real hours), and a one-hour event starting at 01:30 before a spring-forward change was rejected because 02:30 does not exist; both now end exactly the requested duration later.
- 83596ab: `calendar +insert`, `+update` and `+freebusy` no longer crash on a huge `--duration` or `--slot` (for example `99999999999999999d`, or a length that ends past the latest representable date). They now fail with a validation error (exit 3).
- 6581320: `calendar +freebusy` once again uses the account time zone when the profile was granted `calendar.freebusy`. Its token was narrowed to that scope alone, and Calendar refuses that scope for `settings.get`, so the time zone lookup failed and the command fell back to the machine's local zone. That changed how `--start`/`--end` without an offset and the reported free slots were read. The helper now also asks for `calendar.readonly`, which `settings.get` accepts.
- 6e89c27: `calendar +insert` and `calendar +update` now send the time zone with a local (offset-less) end time when the start time has an offset. Previously the end was sent as `"timeZone": null`, so the API rejected the request; this happened for `+insert --start <RFC 3339> --end <local time>` and for `+update --end <local time>` without `--start`, which reuses the event's current start.
- 4cb44d4: `calendar +insert` and `+update` no longer fetch the account time zone when every `--start`/`--end` has a UTC offset or is a date (or only other fields change), so a failing time zone lookup can no longer fail those commands.
- d2a8c73: `calendar +update --dry-run` now lists the GET of the current event that a real run sends before its PATCH when it merges `--add-attendee`/`--remove-attendee` or keeps the event's time zones. The plan also notes that times with an offset keep each end's current time zone.
- bd6491c: `calendar +update` keeps the event's own time zone when it moves the event with times that carry an offset (`--start`/`--end` like `2026-06-17T10:00:00-06:00`, or `--duration` alone) and no `--timezone`. It used to send the new `start`/`end` without a `timeZone`, dropping the event's zone (recurring events need one). Each end now keeps its current `timeZone`; `--timezone` still overrides it.
- dfe0c50: `calendar +update` can switch an event between all-day and timed. The Calendar API merges a patched `start`/`end` into the event's current one, so a new all-day date left the old `dateTime` in place (and a new time left the old `date`). The patch now clears the other field.
- 7ce2582: `gwsr calendar +insert`, `+agenda`, `+freebusy` and `+update` now check `--timezone`, `--start`/`--end`/`--duration`, `--slot` and `--working-hours` (and, for `+update`, that something changes) before asking for confirmation or loading credentials. These now exit with a validation error (3), where they used to exit with a confirmation (7) or authentication (2) error after a time-zone lookup.
- 06f8df1: `drive +sync`, `gmail +attachments` and `script +pull` now treat names that macOS and Windows file systems consider one file, such as `ας` and `ασ` (final and medial sigma) or `ſ` and `s`, as colliding, so one is disambiguated or refused instead of silently overwriting the other.
- 43681b7: The PR policy check now rejects changesets that `changeset version` cannot parse because a package is listed twice or a key's colon has no space after it (`gws-rust:patch`). These used to pass CI and then break the release job on main.
- 35ee305: `gwsr chat +send` now rejects an empty `--text` before asking for confirmation or loading credentials, so it exits with a validation error (3) instead of a confirmation (7) or authentication (2) error.
- f551398: Programs gwsr starts no longer inherit its environment variables. `gcloud` (run by `gwsr auth setup`) and the browser opened by `gwsr auth login` used to receive the whole environment, `GWSR_TOKEN`, `GWSR_CLIENT_SECRET` and the build-time `GWSR_DEFAULT_CLIENT_SECRET` included; every `GWSR_*` variable is now withheld from them, and the rest of the environment is passed on. The browser is now opened with the platform launcher directly (`open` on macOS, `xdg-open` or `wslview` elsewhere on Unix, `rundll32 url.dll,FileProtocolHandler` on Windows) instead of through the `webbrowser` crate, which started some launchers itself with the full environment.
- 6f05f65: Argument errors keep clap's suggestions in the JSON error envelope: a mistyped flag, resource or method now carries a `hint` such as `a similar argument exists: '--params'`, as the human (`--format table`) error already showed.
- 1199ce8: A `client_secret.json` whose `client_id` or `client_secret` is empty or contains whitespace, control or non-ASCII characters (typically a space pasted with the value) is now rejected with an error naming the field and file, instead of being sent to Google and failing as `invalid_client` ("The OAuth client was not found"). The same values are refused when the file is written.
- d42aa1c: A mistyped first word now gets the single closest suggestion across built-in commands and services. `gwsr auht` suggests `gwsr auth` instead of `vault`, and `gwsr hlep` suggests `gwsr help` instead of `keep`; `gwsr drvie` and `gwsr gmial` still suggest `drive` and `gmail`. Commands and services are ranked by the same edit distance, where a transposition counts as one edit, and a tie goes to the service.
- 20ec2cc: `--columns` with the `json` or `yaml` output format (the JSON default included) is now an error (exit 3) instead of being silently ignored. It only shapes `table` and `csv` output; add `--format table` or `--format csv`, or select fields with `--jq`.
- d67bbf2: `gwsr commands` accepts `<api>:<version>` like `schema`, `batch` and normal commands: `gwsr commands drive:v2` lists Drive v2 and `gwsr commands youtube:v3` lists an API that is not in the registry (under its API name, with the Discovery document's description). Both used to fail with "unknown service". An unknown name now gets the same error as elsewhere, with a "did you mean" suggestion.
- 0f99aca: `gwsr commands <SERVICE> --api-version V` now lists the commands of version `V` and reports it as `version`. The flag used to be accepted and ignored, so the inventory showed the default version. `gwsr commands --api-version V` with no service named is now a validation error (exit 3): one version cannot apply to every API.
- 0830911: `gwsr commands` lists a service once when it is named twice or by two of its aliases (`gwsr commands reports admin-reports`). It used to fetch it twice and print a duplicate entry in `services` (a duplicate set of rows with `--format table|csv`).
- 834052f: `gwsr commands` now lists the `auth` subcommands (`auth login`, `auth status`, `auth setup`, ...) with their flags. It used to list a single `auth` command whose only flag was a raw `args` passthrough, so agents reading the inventory could not discover how to log in or check credentials.
- 79cd589: Shell completions now follow the API version you select. `gwsr drive --api-version v2 <TAB>` completes the resources and methods of the cached Drive v2 document instead of the default v3, and `gwsr drive:v2 <TAB>` (or any `<api>:<version>`) completes at all, from the same document the command would load. An invalid version completes nothing instead of falling back to the default version.
- 680d192: Shell completions now complete local paths for flags that take a file or directory (`--upload`, `-o/--output`, `--file`, `--attach`, `--dir`, `--output-dir`, `batch --input` and others). Before, these flags completed nothing, so zsh, fish, PowerShell and elvish offered no file names at all (bash only fell back to its own file completion).
- d4ac546: The config directory, `config.toml` and `profile.json` are no longer read when another user owns them or when group or others may write to them. This matters under a shared location such as `GWSR_CONFIG_DIR=/tmp/gwsr`, where another user could create the directory first and plant a config that picks the profile, log file or Model Armor template. Such a directory or file is now a configuration error (exit 8) that names the path and the `chmod`/`chown` fix. Credential, token, client and key files, which were already refused when group or others could read them, are now also refused when another user owns them. Files owned by root are still accepted.
- c64fe61: Every `config.toml` value is now validated when the file is loaded, whether or not an environment variable or flag overrides it. An invalid value is a configuration error (exit 8) that names the file. Before this fix:
  
  - an invalid `format`, `json_style` or `sanitize_mode` loaded without error while the matching `GWSR_*` variable was set.
  - an invalid `sanitize_template` was only caught when a service command used it, and was then reported as a validation error (exit 3).
  - `log_file` accepted a relative path, so the log directory was created under whatever directory `gwsr` ran in. Like `GWSR_LOG_FILE`, it must now be absolute.
- d4756be: An invalid `profile` in `config.toml` is now a configuration error (exit 8) for every command, as an invalid value of any other key already is. It used to load without error while `--profile` or `GWSR_PROFILE` overrode it, and for commands that load no credentials (`schema`, `cache clear`, `--dry-run`). To fix a file that now fails, set `profile` to a valid name: 1-64 lowercase letters, digits, `-`, `_` or `.`.
- bd212ac: The confirmation gate now treats four irreversible `POST` methods as destructive, so they need `--yes` (or a prompt on a terminal) like any `DELETE`: Admin SDK `customers.chrome.printers.batchDeletePrinters` and `customers.chrome.printServers.batchDeletePrintServers`, and Cloud Identity `devices.wipe` and `devices.deviceUsers.wipe`. Before, they ran without confirmation, unlike `gmail.users.messages.batchDelete` and `people.people.batchDeleteContacts`.
- 3019aee: Internal documentation fixes: the confirmation gate docs now list every accepted `GWSR_REQUIRE_CONFIRM` value (`1`/`true`/`yes`/`on`), and the `gwsr batch` docs list the exact retryable statuses (408, 429, 500, 502, 503, 504, or a 403 rate limit) instead of "5xx". No behavior change.
- cd5d57e: The confirmation gate now looks at the request body of two Admin SDK Directory device commands and treats a wipe as destructive, so it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`: `directory.mobiledevices.action` with `action` `admin_remote_wipe` or `admin_account_wipe`, and `admin.customer.devices.chromeos.issueCommand` with `commandType` `WIPE_USERS` or `REMOTE_POWERWASH`. Before, these ran without confirmation. Other actions of the same methods (approving or blocking a phone, rebooting a Chromebook) are still not gated.
- 79a9c61: The confirmation gate now treats deprovisioning a ChromeOS device as destructive, so it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`: `directory.chromeosdevices.action` with `action` `deprovision`, and `admin.customer.devices.chromeos.batchChangeStatus` with `changeChromeOsDeviceStatusAction` `CHANGE_CHROME_OS_DEVICE_STATUS_ACTION_DEPROVISION`. A deprovisioned device loses its policies, device-level printers and kiosk use, and must be wiped before it can be enrolled again. Before, these ran without confirmation. Disabling and re-enabling a device are still not gated.
- 0808ff5: `admin users update`/`patch` now require confirmation (`--yes`, or a prompt on a terminal) when the body sets `suspended` to `true`, like `admin +user-suspend`; other user edits and unsuspending are not gated. More Classroom methods are now gated as outbound (confirmation required when `GWSR_REQUIRE_CONFIRM=1`), directly or in `gwsr batch`: `classroom invitations create` (it emails the invitee), `classroom courses students create` and `classroom courses teachers create` (they add people to a course), and `classroom courses announcements create` (it posts to the whole class). These commands now take `--yes`.
- 1292bd6: `drive permissions create`/`update` (and Drive v2's `permissions insert`/`update`/`patch`), directly or in `gwsr batch`, are now gated as destructive when they transfer ownership (`transferOwnership`, role `owner` or `pendingOwner`) or give a whole domain or anyone with the link a role above reader/commenter, as `drive +share` already was. They were only outbound, so without `GWSR_REQUIRE_CONFIRM=1` an ownership transfer went through without `--yes`. Drive v2's `permissions insert` and `patch` are now outbound like their v3 counterparts.
- aea219a: `keep notes permissions batchCreate`, which shares a Keep note with other people, is now gated as outbound like Drive's `permissions create`: with `GWSR_REQUIRE_CONFIRM=1` it needs `--yes` (or a prompt on a terminal), directly or in `gwsr batch`. The command now takes `--yes`.
- f87a815: More generated methods that notify people or grant them access are now outbound, so `GWSR_REQUIRE_CONFIRM=1` gates them (directly and in `gwsr batch`) and their commands take `--yes`. The new ones are `chat.spaces.members.create`, `chat.spaces.setup`, `meet.spaces.members.create`, `cloudidentity.groups.memberships.create` (the Cloud Identity counterpart of `directory.members.insert`), `directory.users.makeAdmin`, `directory.roleAssignments.insert` and `gmail.users.settings.sendAs.verify`, which emails the send-as address. Before, they were sent without confirmation.
- 5c20040: More generated methods are now gated as outbound (confirmation required when `GWSR_REQUIRE_CONFIRM=1`), directly or in `gwsr batch`: `script projects deployments create`/`update` (they publish Apps Script code for others to run), `classroom userProfiles guardianInvitations create` (it emails the guardian), `drive approvals comment` (it notifies the initiator and reviewers), `gmail users settings updateVacation` when the body turns the auto-reply on, and `groupssettings groups update`/`patch` when the body lets anyone join or view the group (`whoCanJoin` `ANYONE_CAN_JOIN`, `whoCanViewGroup` `ANYONE_CAN_VIEW`) or allows external members (`allowExternalMembers` `true`). These commands now take `--yes`.
- 6ee74d0: The interactive confirmation prompt (`About to … Proceed? [y/N]`) now strips control characters and escape sequences from the action it names. An argument such as `--event-id` or `--attendee` could otherwise rewrite the question on the terminal, or use OSC 52 to set the clipboard, before the user answered.
- ce834b6: The `gws-rust` and `gws-rust-core` crates on crates.io now include the `LICENSE` and `NOTICE` files, as the Apache-2.0 license requires for redistributions.
- fe7c3a7: `--format csv --columns` now applies to rows that are not objects (Sheets `values`, a list of IDs) instead of silently ignoring the selection and printing every row whole. As with `--format table`, such rows have the single column `value`, and naming any other column is an error (exit 3).
- bcce0b8: Security: `--format csv` now neutralises a spreadsheet formula in a field whose value is an array. An array is written as one cell with its elements joined by `, `, so a value such as `["=HYPERLINK(...)"]` in API data became the cell `=HYPERLINK(...)` without the `'` prefix, and a spreadsheet would run it as a formula. Array cells starting with `-` (for example `[-5, 3]`) now also get the `'` prefix; single numbers are unchanged.
- e712f8c: Security: `--format csv` now neutralises spreadsheet formulas that start after a stripped control or invisible character. Before, a value such as `<BEL>=HYPERLINK(...)` or `<zero-width space>@SUM(...)` in API data had the leading character removed but did not get the `'` prefix, so a spreadsheet would run it as a formula.
- 4a3e757: Paginated `--format csv` and `--format table` output (`--page-all`, `--page-items`) now aligns every row to the header printed with the first page. Previously each page computed its own columns, so a later page whose items had a missing, extra or reordered field printed values under the wrong headers. The first page with rows fixes the columns (the `--columns` selection, or that page's fields); later rows get empty cells for missing fields, and a field that first appears on a later page is left out with a warning on stderr naming it (use `--columns` or `--format json` to keep it). Table widths also stay fixed across pages, and `--page-items` items render as table rows instead of one key/value block per item.
- 6b7fbdd: `--format csv` now quotes a lone scalar value (a `--jq` result such as `--jq .name`, or a string item of `--page-items`) like any other CSV field. Previously it was printed raw, so a value with a comma, a double quote or a line break split into several fields or records.
- 09cc3f2: Tests no longer share one HTTP connection pool across Tokio runtimes. Each `#[tokio::test]` runs its own runtime, and a pooled connection is driven by the runtime that opened it, so wiremock-backed tests (Pub/Sub pull, Workspace Events renew, Calendar, Docs) intermittently failed with "dispatch task is gone: runtime dropped the dispatch task". The CLI itself runs one runtime and keeps its single shared client; `gws_rust_core::client::build_client()` builds an unshared client for callers that run more than one runtime.
- 6870073: When a retried DELETE gets 404 or 410 after an earlier attempt timed out or returned a server error, the error now says that the earlier attempt may already have deleted the resource.
- cf6376d: On macOS and Windows, naming an API with different letter case (`gwsr Drive:v3`, `gwsr youtubeanalytics:v2`) no longer deletes the cached Discovery document of the real API (`drive`, `youtubeAnalytics`) and warns that it is an invalid cache entry. On a case-insensitive file system both names share one cache file, and that file is now treated as a cache miss.
- ed87614: An API name that differs from the real one only in case (`gwsr siteverification:v1`, `gwsr SITEVERIFICATION:v1`) now fails as "No Discovery Document found" (exit 4) with the hint "did you mean `siteVerification:v1`? API names are case-sensitive", instead of an "Invalid Discovery Document" fetch failure. The per-service endpoint is now tried at the lowercase host for any API name, so `gwsr siteVerification:v1` loads. A missing API whose name matches a registered one ignoring case (e.g. `DRIVE:v3`) also suggests the registered name. Lookup itself stays case-sensitive.
- 0abf13d: Methods that a Discovery document flags with `deprecated: true` (for example `admin chromeosdevices action`) are now hidden from help and completions, as the README describes. Only methods whose description starts with "Deprecated" were hidden before. Hidden methods still run when called by name.
- 97c1897: Discovery document fetches honor `GWSR_TIMEOUT` and `timeout_secs` in `config.toml`. They used a fixed 60-second limit, so a shorter timeout did not apply to them and `0` did not disable it. The timeout bounds the wait for the response and each gap while reading the document, as it does for API requests.
- d31a758: APIs whose Discovery document gives a method and a resource the same name at one level (`apigee:v1`, `aiplatform:v1`, `aiplatform:v1beta1`, `connectors:v2`, `identitytoolkit:v1`), or names one `help`, no longer crash `gwsr` while it builds the command tree. Before this fix, one of the two commands also became unreachable. Now every method has a command. The method keeps the shared name and the resource becomes `<name>-resource`, for example `gwsr apigee:v1 organizations developers attributes-resource list`. A method or resource named `help` becomes `help-method` or `help-resource`. `gwsr schema` and `gwsr batch` accept the Discovery path or the command path, and `gwsr commands` and completions list the command names.
- 9846af4: Discovery methods declared at the top level of a document, outside any resource (for example `oauth2:v2`'s `tokeninfo`), are now commands (`gwsr oauth2:v2 tokeninfo`), schema entries (`gwsr schema oauth2:v2.tokeninfo`) and `gwsr batch` targets. They used to be dropped without any message. Their paths go through the same endpoint-safety checks as resource methods.
- a32fdd5: An invalid Discovery cache entry that cannot be deleted no longer fails the command: it is ignored with a warning and the document is fetched again.
- 79aaecd: Fixed documented examples that `gwsr` rejects: the Model Armor helper examples (and the skills generated from them) and the README `--sanitize` example now use a valid project ID instead of `projects/P/...`; the README `calendar +freebusy` example uses `--end` (an all-day `--start` does not accept `--duration`); the README `drive +upload --convert` example uploads a `.docx` (PDFs have no Google format to convert to); and the README `modelarmor +create-template` mention includes its required flags. A new test runs every `gwsr` example in the README, CONTEXT.md and the skills with `--dry-run` so a rejected example fails CI.
- 83fd4a6: `docs +create --text`/`--text-file`: when the document is created but writing its content fails, the error now names the new document (id and URL) and says to add the content with `docs +write --document-id`, instead of a bare API error that made a rerun of `+create` leave an empty duplicate document behind. The error keeps its class and exit code.
- 59373e5: `docs +write --markdown` and `docs +create --markdown` no longer add blank lines inside a code block that belongs to a list item when the Markdown has Windows (CRLF) line endings. Each code line used to be followed by an empty line, and the item ended with a stray line break.
- dce6ea1: `docs +write --markdown` and `docs +create --markdown` now link an email autolink such as `<ann@example.com>` to `mailto:ann@example.com`. Before, the link URL was the bare address, which is not a mail link.
- ddc4ea6: `docs +write --markdown` no longer carries the text style (size, color) and paragraph layout (quote indent, rule border, alignment) of the paragraph it is appended to into the new content.
- 79bbfa5: `docs +write --markdown` and `docs +create --markdown` no longer lose a heading inside a list item that is followed by more of the item, as in `- # Title` then an indented paragraph. The heading's paragraph was dropped from the write plan, so its text was inserted without the heading style and the list around it was split in two.
- 70b37f6: `docs +write --markdown` and `docs +create --markdown`: a list item that opens with a block quote (`- > text`) no longer starts with an empty line in Docs.
- 66736bf: `docs +write --markdown` and `docs +create --markdown` keep multi-line list items in one bullet. A second paragraph inside a list item used to become an extra bullet (so `1. a` / `more` / `2. b` numbered three items), and a hard line break or code block inside an item split the list so the following items restarted at 1. These now continue the item on a new line of the same bullet.
- 673961a: `docs +write --markdown` no longer starts a nested list item that opens with a code block with an empty line.
- c46e6c1: `docs +write --markdown` and `docs +create --markdown` no longer misplace formatting after a control character (U+0000–U+0008, U+000C–U+001F) or a Private Use Area character (U+E000–U+F8FF). The Docs API drops these from inserted text, so every bold, italic, link, code, heading and list range after one landed on the wrong characters; they are now left out of the inserted text and its offsets.
- 64304af: `docs +read` Markdown output no longer corrupts text that Google Docs splits into several runs with the same bold, italic, strikethrough, code or link style: they are wrapped once (`*xy*`), not per run (`*x**y*`, which shows literal asterisks and can swallow inline code).
- ae00090: `docs +read` now reads every tab of a multi-tab document, child tabs included. It requests `includeTabsContent=true`; before, the Docs API returned only the first tab and the other tabs' text was silently dropped. With more than one tab, the text and Markdown output start each tab with a `=== Tab: Parent > Child ===` line, and the JSON output gains a `tabs` list (`tabId`, `title`, `parentTabId`). `--body-format raw` now prints the document with its content under `tabs` rather than `body`. The `+write` and `+replace` help now states their tab behavior: `+write` appends to the first tab, and `+replace` replaces in every tab.
- bdb8951: `docs +read --body-format markdown` no longer puts a literal `\` inside code: a line break (Shift+Enter) in monospace text now ends the code span and the next line starts a new one, instead of reading as `a\ b`.
- c1e0868: `docs +read` Markdown keeps monospace text that contains backticks as one code span. Before, ``a`b`` in a code font became `` `a`b` ``, which Markdown reads as the code `a` followed by a literal ``b` ``.
- d70cc4b: `docs +read` Markdown output keeps line breaks inside a paragraph or list item (Shift+Enter) as hard breaks instead of soft breaks that join the lines when rendered or written back.
- 9f33c22: `docs +read` Markdown keeps a heading or title that contains a line break (Shift+Enter) in one heading, writing the break as a space. Before, the break was written as a hard break, which an ATX heading cannot hold: the heading ended with a literal `\` and the text after the break became a separate paragraph.
- 577e95a: `docs +read` Markdown output drops the leading tabs and spaces of a paragraph instead of emitting them, so indented paragraphs are no longer turned into code blocks that show escaped text (`snake\_case`) verbatim.
- b5b3202: `docs +read` Markdown output now leaves a blank line after a list, so the paragraph or table that follows is no longer merged into the last list item.
- 1d734df: docs `+read` (Markdown, the default body format) now escapes document text that looks like Markdown. A paragraph such as `# of seats: 12`, `1. …`, `- 5 degrees` or `f(*args, **kwargs)` used to come out as a heading, a list or emphasis, and inline tags like `<b>` as raw HTML; they now read back as the literal text. Real bold, italic, links and code runs are still rendered as Markdown.
- 5c9b26b: `docs +read --markdown` now indents nested list items to their parent's content column (three spaces under a numbered item, two under a bullet), so items nested under a numbered item stay nested when the Markdown is parsed again.
- 0eda173: `docs +read` Markdown no longer leaves a literal `\` in a table cell whose text has a line break (Shift+Enter). The break was written as a hard break, but a table cell is one line, so its lines are joined with a space and the `\` stayed in the cell (`| one\ two |`). It is now `| one two |`.
- bd62a6e: `docs +read` now includes a table of contents' entries, in place, in Markdown and plain-text output. Before, the whole `tableOfContents` element was dropped from the rendered text, although its entries are document text.
- 462751b: `docs +read --body-format text` now numbers ordered list items the way the document shows them (`1.`, `2.`, `3.`). Numbering is per list and nesting level, and a sub-list restarts under each new parent item. Every item used to read `1.`, and plain text has no renderer to correct that. Markdown output keeps `1.`, which CommonMark numbers itself.
- a18acee: `docs +read --markdown` no longer ends a paragraph with a literal `\` when its last line break (Shift+Enter) is followed only by spaces or tabs. The trailing break is dropped, as it already was when nothing followed it.
- bad13ab: `gwsr docs +create --markdown` no longer creates an empty document before rejecting unsupported Markdown (tables, images, HTML...). The Markdown, `+read --output` (including `--body-format raw` with `--output`) and an empty `+replace --find` are now checked before credentials are loaded, so they exit with a validation error (3) instead of an authentication error (2).
- 2962091: `docs +write --markdown --dry-run` now lists the `documents.get` read that a real run sends to find the end of the document, before the `batchUpdate`. The plan previously showed only the write.
- 16d9056: Refuse request URLs that hide a `.` or `..` path segment behind a backslash or an ASCII tab/CR/LF (for example `files/F1/..\x`), which URL parsing would otherwise silently remove, retargeting the request.
- d7f55d2: `drive +upload --convert` now accepts every Office type Drive imports into Docs, Sheets or Slides, including macro-enabled, template and slideshow files (`.xlsm`, `.xltx`, `.docm`, `.dotx`, `.pptm`, `.potx`, `.ppsx` and so on), which it used to refuse with "no Google format accepts". A `--mime-type` given with different case or with parameters (`text/csv; charset=utf-8`) is matched like the plain type instead of being refused.
- 993d40f: `drive +download` without `--output` saves a Drive file named `-` to a local file instead of streaming its bytes to stdout followed by a JSON summary.
- 573092b: `drive +download --dry-run` and `drive +export --dry-run` now list the `files.get` metadata read that a real run sends before downloading or exporting the content. The dry run still sends no request.
- b4176e5: `drive +download` on a Google-native file no longer always points at `+export`. A shortcut now names its target (`+download --file-id TARGET`). A type that cannot be exported (Forms, Sites, folders and so on) is reported as such. Exportable types list their valid `--to` formats.
- dfee8de: `drive +download --dry-run` and `drive +export --dry-run` without `--output` now show the local file as `<file name>` (or `<file name>.pdf` and so on) in the current directory, since a real run names it after the Drive file. They used to plan a made-up `download` or `export.pdf`, and failed with "--output '…/download' is a directory" when a directory of that name existed, though no `--output` was given.
- ba24407: `drive +export` without `--output` keeps the format's extension (e.g. `.pdf`) when it shortens a long Drive file name for the local file, instead of cutting the extension off.
- 7efb60b: `drive +export` of a shortcut now names the shortcut's target (`+export --file-id TARGET --to FORMAT`) instead of sending you to `+download`, which refuses shortcuts. A Google-native type that cannot be exported (Forms, Sites, folders and so on) is reported as such, without the `+download` hint; regular files are still pointed at `+download`.
- 89290ca: `drive +download` and `drive +sync` keep a file's extension when they shorten a long Drive name for the local file. Before, a name longer than 200 bytes was cut at the end, so `<long name>.pdf` was saved without `.pdf`; this also hit `+sync`'s ID-prefixed names for same-named files, including exported `.docx`/`.xlsx`/`.pptx` copies.
- 96dc760: `drive +move --dry-run` now lists the two `files.get` reads that a real run sends before its PATCH: one for the file's current parents and one for the destination folder's real ID. The dry run still sends no request.
- 927251b: `drive +move --folder-id root` now resolves `root` to the real My Drive folder ID before comparing it with the file's parents. A file already only in My Drive is refused with "File is already in folder root" instead of a request that adds and removes that same folder, and a file in My Drive and another folder only loses the other folder.
- 3949f2e: `gwsr drive +share` now rejects `--message` with `--domain`/`--anyone`, `--role owner` for a group or domain, and `--role owner --no-notify` (Drive requires the notification) before asking for confirmation or loading credentials, and `drive +download`/`+export` check an explicit `--output` first, so these all exit with a validation error (3) instead of a confirmation (7) or authentication (2) error.
- 9f5c622: `drive +download` and `drive +export` of a shortcut now suggest the command that fits the target's type (`shortcutDetails.targetMimeType`): `+export` for a Google Doc, Sheet, Slides or Drawing (with the requested format only when it fits), `+download` for a regular file, and neither for a target that cannot be downloaded or exported. They used to echo the command that was run, so a shortcut to a Doc pointed at `+download` and one to a PDF at `+export`, both of which fail.
- f457e34: `drive +download --output -` and `drive +export --output -` no longer print a JSON summary after the raw payload on stdout, so piping the file (e.g. `| sha256sum`) gets exactly its bytes.
- ff94cf5: `drive +sync` no longer drops a file whose name differs from a sibling only by letter case (for example `Report.txt` and `report.txt`). On case-insensitive file systems (the macOS and Windows defaults) both mapped to one local file, so the second was silently reported as `unchanged` or overwrote the first. Such names now count as a collision, so each is saved as `<file id>-<name>`, like other same-name siblings.
- 10944a2: `drive +sync` on a case-insensitive file system (macOS, Windows) no longer reports a false "untracked" conflict when a new Drive item takes over a synced file's name with only a difference in case, such as `Report.txt` replaced by `report.txt`. If the local file is untouched, it is replaced, the same as a replacement under the exact same name. If it was edited, it is kept and reported as `modifiedLocally`. On a case-sensitive file system the two names are still separate files.
- ab81d75: `drive +sync` now creates a local directory for every Drive folder it mirrors. Before, a directory was only created when a file was downloaded into it, so empty folders, folders holding only empty sub-folders and folders of skipped items (Forms, shortcuts...) were missing from the mirror.
- d152c60: `drive +sync` no longer merges sibling Drive folders that share a name, or a folder and a file with the same name, into one local path (which lost files and reported them as "unchanged"). Files and sub-folders in a Drive folder now share one set of local names: when more than one item maps to the same local name, every one of them is saved as `{id}-{name}`, so the layout no longer depends on the order Drive lists items in. This changes local paths for same-named files: previously the first one listed kept the plain name. An earlier sync's plain-named copy of such an item is left in place and is no longer updated. If an ID-prefixed name still collides with another item's real name, `+sync` fails before downloading anything from that folder instead of overwriting a file.
- 6d459ab: `drive +sync` caps the length of a local file name after Unicode normalization, not before. NFC can double a name's UTF-8 length (U+0958 and other composition exclusions decompose), so a long Drive name could become a local name over the 255-byte limit of ext4, and the sync aborted when it could not create the file.
- 5e05e79: `drive +sync` no longer stops at the first file the API refuses, such as a file whose owner disabled downloads or a Doc too large to export. Before, the whole sync aborted there: it printed nothing about the files it had already written, and every later run stopped at the same file. Now such files are listed under a new `"failed"` key (item id, local path and the API error), the rest of the folder is still mirrored, and the report is printed. The command then exits `1` with reason `syncPartialFailure`, the same way `gwsr batch` reports `batchPartialFailure`. Auth, network and local write errors still abort the sync.
- eea2828: `drive +sync` no longer trusts local modification times. It used to treat a local file as current whenever its mtime was newer than Drive's, so a Drive file replaced by one with an older `modifiedTime` (a different file of the same name moved into the folder, or a revision uploaded with its original timestamp) left a stale copy reported as `unchanged`, and a local edit was silently overwritten once Drive changed.
  
  `+sync` now keeps a sync-state file, `<dir>/.gwsr-sync.json`, keyed by Drive file ID: the Drive `modifiedTime`, `version`, size and checksums last mirrored, and the local size, mtime and SHA-256 written. A file is downloaded again when its Drive copy changed or another Drive item took its name, and only if the local copy is unchanged since the last sync. A local file that was edited, or that the sync did not write, is kept and listed under the new `conflicts` key (reason `modifiedLocally` or `untracked`); the command then exits `1` with reason `syncConflict` after printing the report. The new `--overwrite` flag replaces such files with the Drive copy.
  
  Behavior change: on the first run after upgrading there is no state file, so existing local files are reported as `untracked` conflicts unless Drive's SHA-256 checksum shows they are identical (Google Docs exports have none). Run once with `--overwrite` to take the Drive copies, or move the files aside. A corrupt state file, or one for another folder, is reported on stderr and treated the same way.
- 9c85c8e: `drive +sync` now fails with a validation error when `--folder-id` is a folder in the trash. Before, it listed nothing (everything in a trashed folder is trashed too) and reported success with an empty mirror.
- 710e63d: `drive +sync` no longer loses a file when two Drive names in one folder differ only by Unicode normalization (for example a precomposed `café.txt` and a decomposed `café.txt`). macOS file systems treat them as the same file, so the second was reported `unchanged` and never downloaded. Local names are now NFC-normalized, and the second item is saved with its ID prefix like other same-name items.
- 3975470: `drive +upload --convert` now refuses a file type no Google format imports while parsing its flags, before credentials load, so the mistake exits 3 (validation) instead of 2 (auth) when no one is signed in.
- 2cda797: `drive +upload --mime-type` is now validated before credentials are loaded: a value with control characters (such as an embedded newline) or that is not a `type/subtype` media type is a validation error (exit 3). It used to be sent as the `X-Upload-Content-Type` header and fail only when the request was built, as an internal error (exit 5).
- 607c237: `gwsr workflow +email-to-task` now decodes HTML entities in the Gmail snippet before using it as the task notes, so notes read `Tom & Jerry's` instead of `Tom &amp; Jerry&#39;s`.
- de6ff19: An empty `--fields` mask is now rejected (exit `3`) before anything is sent. Previously it was sent as an empty `fields` parameter, and with `--page-all` or `--page-items` it became the malformed mask `nextPageToken,`.
- 8d5f87b: A POST/PATCH whose `requestId` (in `--params`, the body or a `gwsr batch` line) is empty, blank or null is no longer treated as safe to retry. Such an id gives the API nothing to de-duplicate on, so retrying after a 503 or a dropped connection could apply the call twice, for example by posting a Chat message twice. Only a non-blank `requestId` makes the call retryable.
- 0dfbd73: `--upload` now enforces the method's Discovery upload limits before sending anything: a file larger than `mediaUpload.maxSize` (binary units, e.g. 35 MiB for `gmail users messages send`) or with a media type outside `mediaUpload.accept` (e.g. `text/plain` where Gmail accepts only `message/*`) fails with a validation error (exit 3), also under `--dry-run`, instead of being uploaded in full and then rejected by the server. Set the media type with `--upload-content-type` when detection picks the wrong one. The Gmail helpers (`+send`, `+reply`, `+reply-all` and `+forward`, including drafts) likewise refuse an encoded message over Gmail's 35 MiB limit before sending it.
- cee063e: An invalid `GWSR_API_BASE_URL` that contains `user:password@` no longer repeats the password in the configuration error.
- 661c65f: The JSON error envelope on stderr now escapes C1 control characters (U+0080–U+009F), DEL and bidi, zero-width and line-separator characters as `\uXXXX`. JSON already escaped ESC, but these characters were written raw, and some terminals act on them: U+009B is an 8-bit CSI. Error messages carry API- and filter-derived text such as a `--jq` `error(...)` value. The decoded JSON is unchanged. In the human error form (`--format table|yaml|csv`), argument-parsing errors are now sanitized like every other error, because clap quotes argument values verbatim. As a result, those errors are no longer colored.
- 2fb346e: `events +subscribe`, `events +renew` and `gmail +watch` now send their Workspace Events and Pub/Sub requests to the `GWSR_API_BASE_URL` override when it is set. They used to send them to Google's hosts.
- cb907b6: `gwsr events +renew --subscription-id ID --event-types ... --dry-run` no longer shows a `filter` in its plan. A named subscription is renewed directly without listing subscriptions, so `--event-types` only selects the OAuth scope there. The filter is now shown only with `--all`.
- e2ec0de: `events +renew` no longer reports a subscription as renewed when the API returns a long-running operation that already finished with an error. The renewal (or `--reactivate`) now fails with the class of the operation's status code (for example exit 1 for PERMISSION_DENIED, exit 6 for UNAVAILABLE), alone or as one of the `--all` failures.
- f090bd3: `events +renew` no longer reports a failed renewal as an internal error (exit 5): a single `--subscription-id` exits with the API error's own code (for example 6 for a retryable 503, or 1 for a 404), and a partial `--all` failure exits 1 with reason `renewPartialFailure`.
- e232bda: `events +renew --all --reactivate` now reactivates every SUSPENDED subscription for the given event types. It used to pick subscriptions by expiry time, so it called `:reactivate` on active subscriptions (which the API rejects, failing the command) and skipped suspended ones that were not about to expire. `--within` applies only to renewals.
- c4deb60: `events +subscribe` now deletes the Pub/Sub topic and subscription it just created if a later setup step fails, for example when the Workspace Events API rejects the subscription. Before, the command exited with an error and left both resources behind, even with `--cleanup`. Nothing publishes to them, and no reconnect hint named them. If the Workspace Events operation is still running, the resources are kept.
- 0aced50: The `persona-exec-assistant` skill no longer says `workflow +meeting-prep` shows "linked docs". It shows the next meeting's attendees, description, location and Meet link; no command lists a meeting's documents.
- b871bfe: Generated commands now ask for JSON from APIs whose Discovery document defaults to another response format. The Groups Settings API (`gwsr groupssettings groups get|update|patch`) defaults `alt` to `atom`, so its responses came back as Atom XML instead of JSON. `gwsr` now sends `alt=json` when the document's default `alt` is not `json` and the caller did not set `alt` in `--params`. `--dry-run` shows the added parameter.
- 95b5cdc: A generated method that changes something and whose decoded base64 payload cannot be written to stdout (`-o -`, for example on a full disk) now says in the error that the change was made and names the resource, as the JSON and `-o FILE` paths already did. Before, the error read as if nothing had been sent, so a retry repeated the write.
- 5f01a01: Generated method commands and `gwsr batch` now load credentials only after validating `--params`/`--json`/`--upload` and passing the confirmation gate. Bad input is a validation error (3) and a refused destructive call is a confirmation error (7), even when credentials are broken or a token would need a refresh; before, these were credential errors (2 or 8) and could trigger a token refresh first.
- 7bec770: `gwsr auth export --unmasked --output FILE` no longer leaves a half-written secrets file behind when a write or fsync fails. The credentials are written to a private temporary file in the same directory and moved into place only after they are fully written and synced, so a retry is not refused because of the partial file. Such a failure is now reported as an I/O error (exit 5) instead of a validation error (exit 3); an existing FILE is still refused with exit 3.
- 0c6fe50: Table and CSV output no longer drop a value when a field name contains a dot. A literal `"a.b"` key and a nested `{"a": {"b": …}}` used to flatten to the same `a.b` column, and one of the two values was silently lost. Dots and backslashes inside field names are now escaped in column names (`a\.b`, `a\\b`), and `--columns` accepts the escaped form. Column headers for such fields change accordingly.
- 2059f19: Table and CSV output no longer drop a value when a field name is empty. A nested `{"": {"a": 1}}` used to flatten to the column `a`, the same as a sibling `"a"` field, and one of the two values was silently lost. An empty field name is now kept as a path segment, so the nested value is the column `.a`.
- 62260b0: When `gwsr` reads the body of an HTTP 403 to check for a rate-limit reason, `--timeout` now limits each gap between chunks, as it does for every other response body (and as the README says). Before, it limited the whole read, so a 403 body that arrived slowly but steadily failed with a false "no data received" error.
- 0c7c02a: A 403 response whose body stops arriving now fails after `--timeout` (exit 10) instead of hanging forever while gwsr checks it for a rate-limit reason.
- 61dd289: `--format csv` writes a rectangular CSV for arrays of rows, such as Sheets `values` from `sheets +read` or `spreadsheets values get`. The Sheets API drops trailing empty cells and returns an empty row as `[]`, so rows used to have different field counts, and strict CSV readers such as pandas rejected the file. Each row is now padded with empty fields to the width of the widest row.
- c3be1ef: `forms +responses --output` now guards its CSV against formula injection, like `--format csv` does: a response or question title starting with `=`, `+`, `-` or `@` gets a leading `'`, so a respondent's answer such as `=HYPERLINK(...)` no longer becomes a live formula when the exported file is opened in a spreadsheet. JSON output is unchanged.
- 3dedd99: `forms +responses` no longer drops answers when two questions have the same title, which is common in multi-section forms, or when a question is titled like a fixed column such as `responseId`. The JSON rows are keyed by column header, so the later answer used to overwrite the earlier one. A repeated title now gets its question ID appended, for example `Comments (1a2b3c4d)`, in both the JSON and CSV headers.
- 169ee73: `gmail +forward --body` now keeps the note verbatim, as `+send` and `+reply` keep `--body`. Before, leading and trailing whitespace and blank lines were trimmed, which lost the indentation of a first line or a list. A blank note is still treated as no note.
- d8832bb: `calendar +freebusy --slot --working-hours` no longer drops a whole day when the working-hours start or end falls in a daylight-saving gap (for example 02:00 on a spring-forward day). The time now shifts to the first valid local time after the gap.
- a57878d: Helper options that take free text accept a value that starts with `-`. Before, `docs`/`sheets`/`slides +create --title '-draft'`, `sheets +create --sheet`, `gmail +search --query '-in:spam'`, `gmail +send --subject`/`--body`, `gmail +filter create --from`/`--to`/`--subject`/`--query`/`--exclude`, `calendar +insert`/`+update --summary`/`--location`/`--description`, `calendar +agenda --calendar-name`, `calendar +rsvp --comment`, `chat +send --text`, `tasks +add --title`/`--notes`, `drive +upload --name`, `drive +share --message`, `people +find --query`, `admin +user-create --given-name`/`--family-name`, `modelarmor +sanitize-prompt`/`+sanitize-response --text` and `workflow +file-announce --message` rejected such a value as an unknown flag.
- c3916b5: `calendar +freebusy` now fails when a `--calendar-id` group cannot be expanded (for example `groupTooBig`), instead of reporting free slots that silently leave out every member of that group. Transient expansion errors (`backendError`, `internalError`) exit 6 like per-calendar ones.
- a104c4a: `calendar +freebusy` no longer exits `6` (retry later) when one calendar hit a transient Google error but another failed permanently (for example `notFound`, a calendar you can't see). Retrying can't fix the permanent failure, so the run now exits `3` and names every failed calendar. It still exits `6` when every failure is transient (`backendError`, `internalError`).
- 6f3999d: `calendar +freebusy` now exits `6` (retryable) when Google reports a per-calendar `backendError` or `internalError`, instead of exit `3` (validation error).
- b331507: The generated `sheets spreadsheets values clear`, `batchClear` and `batchClearByDataFilter` methods and `script projects updateContent` now need `--yes` (or a confirmed prompt) like other destructive methods. They erase data just as the already-gated `sheets +clear` and `script +push` helpers do, but ran without confirmation.
- fce95f9: When `gcloud auth print-access-token` fails during `gwsr auth setup`, the error shows gcloud's stderr only, never a token it may already have printed to stdout.
- d48f3a8: `gwsr dev generate-skills` writes each `SKILL.md` and the `--index` file atomically, and replaces a symlink at a skill's path instead of writing through it, so a symlink in `--output-dir` can no longer overwrite the file it points to (or, with `GWSR_RESTRICT_PATHS=cwd`, reach outside the current directory: that is now refused). Every skill and the index are rendered, and every `--filter` checked, before anything is written, so a failed run (an unmatched `--filter`, a Discovery fetch error) no longer leaves some skills written.
- d57adb5: Gmail helpers now handle RFC 5322 comments in addresses. A message whose sender uses the legacy `alice@example.com (Alice Smith)` form is replied to at `alice@example.com`, not at the invalid address `alice@example.com (Alice Smith)`. A comma inside a comment, as in `bob@example.com (Smith, Bob)`, no longer splits one recipient into two broken ones in `+reply-all` Cc lists or in `--to`/`--cc`/`--bcc`. Parentheses that are not comments, such as `Malo (Work) <malo@example.com>` or an emoticon in a display name, are kept as they are.
- a88c309: Gmail helpers (`+send`, `+reply`, `+reply-all`, `+forward`, `+read`, `+triage`, `+search`, `+label`, `+archive`, `+trash`, `+filter`, `+unsubscribe`, `+attachments` and the Gmail calls of `+watch`) now follow `GWSR_API_BASE_URL`, both in real runs and in their `--dry-run` plans. Before, they always went to `https://gmail.googleapis.com`, ignoring the configured endpoint that generated Gmail methods and the other helpers use.
- 20297da: Gmail `--attach` names (and types) a symlinked file after the path you gave, not after the file the link points to.
- 28e979e: `gmail +attachments` no longer gives a long attachment name a trailing dot or space when it shortens it to 200 bytes; Windows drops them, so the file ended up with a different name than the one reported.
- f3769d5: `gmail +attachments` refuses an attachment whose target in `--output-dir` is an existing directory before downloading anything, with or without `--overwrite`. Before, it wrote the attachments listed before that one and then failed part-way.
- 85771eb: `gmail +attachments` no longer skips files such as PDFs that the sender marked inline with a Content-ID (as Apple Mail does); only inline images need `--include-inline`.
- bbe62e5: When `gmail +attachments` fails part-way, the error now lists the attachments it already wrote (or says none were written) and keeps its exit code.
- fe03832: `gmail +attachments` now keeps apart attachment names that differ only in Unicode normalization, such as a precomposed `café.pdf` and a decomposed `café.pdf`. macOS treats the two spellings as one file, so the second attachment used to replace the first under `--overwrite` (or fail halfway without it), while the output still listed both. The second one is now saved as `café (1).pdf`.
- 2c81ec1: `gmail +label` and `+archive` split more than 1000 message IDs into several `batchModify` requests (Gmail's per-request limit) instead of failing with HTTP 400.
- cb1666b: Gmail `+send`, `+reply`, `+reply-all` and `+forward` send a bare carriage return in `--body` (or in a quoted original) as a line break. It was written into the message as a lone CR, which RFC 5322 does not allow, so the line break was lost.
- 30d2a0d: `gmail +read`, `+reply`, `+reply-all`, `+forward` and `+attachments` now fetch a large message body that the Gmail API returns by attachment ID instead of inline. Before, such a body was treated as a file: `+read` printed the truncated API snippet, replies and forwards quoted only the snippet, `+forward` attached the body as `part-0.html`, and `+attachments` saved it as one. If the body can't be fetched, the command fails with the API error instead of falling back to the snippet.
- d78bf85: Gmail `+read`, `+reply`, `+reply-all` and `+forward` remove a byte-order mark from a message body instead of keeping it as an invisible U+FEFF at the start of the text (and of the quoted original), and decode a `charset=utf-16` body in the byte order its BOM gives. A big-endian UTF-16 body was read as little-endian and came out as unrelated CJK characters.
- 1b4ad66: `gmail +read` (and the quoted original in `+reply`/`+forward`) now decodes a message body in the charset its `Content-Type` names. The Gmail API returns body bytes in their original charset, so ISO-8859-1, windows-1252, Shift_JIS and other non-UTF-8 bodies used to come out with U+FFFD in place of every non-ASCII character. A missing or US-ASCII charset is still read as UTF-8, and an unknown charset falls back to UTF-8 with a warning.
- dd7becf: `gmail +read`, `+reply`, `+reply-all` and `+forward` now find a message body whose part type is written in upper or mixed case, such as `TEXT/PLAIN` or `Text/Html`. MIME types are case-insensitive, but these parts were skipped, so the body fell back to the truncated API snippet.
- 380e7f1: `gmail +read`, `+attachments`, `+reply` and `+forward` no longer use a text part that isn't the message's body as the body. An unnamed text part marked `Content-Disposition: attachment` is now treated as an attachment. Before, it could replace the real (HTML) body and was missing from `+attachments`. The text of an embedded `message/rfc822` message is no longer read as the outer message's body. An unnamed `text/plain` attachment is saved as `part-N.txt`.
- 165ae8f: Gmail `+read`, `+reply`, `+reply-all` and `+forward` decode bodies labelled with common charset aliases that are not WHATWG labels, such as Windows code page names (`cp932`, `cp936`, `cp949`, `cp950`, `cp874`, `cp65001`, `cp2859x`) and `latin-1` / `latin9`. These were decoded as UTF-8 and came out garbled. Charsets with no decoder (`utf-7`, `cp437`, `cp850`) are still decoded as UTF-8 with a warning.
- 1e54ff0: Gmail `+read`, `+reply`, `+reply-all` and `+forward` ignore a comment in a body part's `Content-Type`. `charset=iso-8859-1 (latin)` was read as the unknown charset `iso-8859-1 (latin)`, so the body was decoded as UTF-8 and its accented characters came out as `�`.
- 096776d: Gmail helpers no longer corrupt display names that contain escaped quotes or backslashes. Replying to `"Bob \"The Builder\"" <bob@example.com>` used to send the reply to `"Bob \\\"The Builder\\\""`, so every round trip added more backslashes. The recipient now sees `Bob "The Builder"`. A display name like `"A" and "B"` is also no longer turned into `A" and "B`.
- 3d20db6: `gmail +send`, `+reply`, `+reply-all` and `+forward` no longer base64-encode a `message/rfc822` attachment (an attached `.eml`, or an attached message carried over by `+forward`), which RFC 2046 §5.2.1 forbids. A message with CRLF line endings, lines of at most 998 octets and no NUL is sent unencoded (`7bit`, or `8bit` when it is UTF-8). Any other file is sent byte for byte as `application/octet-stream` under the same name.
- 2927553: `gmail +filter create --forward` is now an outbound action: with `GWSR_REQUIRE_CONFIRM=1` it needs `--yes` (or a prompt on a terminal), like `gmail +forward`. Previously a filter that automatically forwards matching mail was created without confirmation. `+filter create` now accepts `-y/--yes`.
- 6768761: `gmail +filter create` with `--add-label`, `--remove-label` or a label flag such as `--archive` or `--star` no longer fails with "insufficient scopes" when the profile was granted `gmail.settings.basic` (for example by `gwsr auth login --full`). The labels used to resolve label names are now listed with their own token (`gmail.readonly`, or every granted scope when that is not in the grant), because `users.labels.list` does not accept the token narrowed to `gmail.settings.basic` that creates the filter.
- 790eeac: The `gmail +filter` help tip now names a login that grants the scope it needs: `gwsr auth login --scopes gmail.settings.basic`. It used to suggest `gwsr auth login -s gmail`, which requests only read-only Gmail scopes without a terminal, so `+filter` still failed for lack of `gmail.settings.basic`.
- c3e247f: `gmail +forward` keeps the `charset` of the original message's attachments. Gmail reports a part's type without parameters, so a forwarded `notes.txt` sent as `text/plain; charset=windows-1252` went out as plain `text/plain`, which mail clients read as US-ASCII and show garbled. A charset that is not a valid MIME token is left out, with a warning.
- 40c908a: `gmail +forward` no longer drops a PDF or other non-image file that the original sender marked `Content-Disposition: inline` with a Content-ID (as Apple Mail does) from a plain-text forward, and `--no-original-attachments` now leaves it out like any other attachment. In an HTML forward such a file is a regular attachment instead of a part beside the HTML body, and `+reply --html` / `+reply-all --html` no longer re-attach it to the reply. Only inline images count as inline, as for `+attachments`.
- d11863c: Gmail `+forward` passes on a meeting invitation sent as an unnamed `text/calendar` part, attached as `invite.ics`. Before, the part was neither the body nor an attachment, so it was dropped. `+attachments` and `+read` now list it too. An unnamed calendar part that Gmail returned by reference was named `part-N.calendar`; it is now also `invite.ics`. When the message also has a named `.ics` attachment with the same invitation, as Google Calendar sends it, only the named one is kept, so the invitation is not forwarded twice.
- a758f1d: Gmail `+reply`, `+reply-all` and `+forward` now read the original message's headers case-insensitively, as RFC 5322 requires. Messages whose sender wrote headers such as `CC:`, `Reply-to:` or `Message-id:` previously lost their Cc recipients on reply-all, ignored the Reply-To address, or failed with "Message is missing Message-ID header"; they are now handled like any other message.
- 9b23936: Gmail `+send`, `+reply`, `+reply-all` and `+forward` once again accept the zero-width joiner and non-joiner (U+200D, U+200C) in `--subject` and in display names in `--to`, `--cc`, `--bcc` and `--from`. Emoji sequences such as 👨‍👩‍👧 and Persian or Indic text use these characters, and they had been refused with exit 3. Other invisible and bidi characters are still refused, and an email address may not contain any of them, joiners included.
- 454b823: `gmail +label`, `+archive`, `+trash`, `+filter list`/`delete` and `+resolve-url` now apply `--sanitize` (and `GWSR_SANITIZE_TEMPLATE`) to their output. They used to ignore it silently and print unscreened results even in `block` mode.
  
  `gmail +send`, `+reply`, `+reply-all` and `+forward` (also with `--draft`) now screen the outgoing subject and body, including quoted or forwarded text, with Model Armor **before** sending. In `block` mode a match (exit `11`) or a failure to reach Model Armor (the cause's exit code) sends nothing; in `warn` mode the message is sent after a warning and the printed result carries `_sanitization`. `+filter create` screens the filter before creating it and `+unsubscribe` screens the unsubscribe URL before the one-click POST, with the same rules. `--dry-run` does not call Model Armor.
- 14ad5c4: `gmail +label --dry-run` and `gmail +filter create --dry-run` now list the `users.labels.list` read (`GET .../users/me/labels`) that a real run sends first to resolve label names to IDs. `+filter create` lists it only when the filter adds or removes labels (`--add-label`, `--remove-label`, `--archive`, `--star` and the like), as a real run does. The dry run still sends no request.
- c9c6759: `gmail +label`, `+filter create --add-label/--remove-label` now match label names case-insensitively for non-ASCII letters too, as documented: `--add 'follow-up/ünï'` finds the label `Follow-up/Ünï` instead of failing with "Unknown Gmail label(s)".
- 5931e60: Gmail `+reply`, `+reply-all` and `+forward` no longer take an address from a `<...>` inside a comment or a quoted display name. `a@example.com (Support <team>)` was read as the address `team`, and `Alice <alice@example.com> (via <relay@example.net>)` as `relay@example.net`, so a reply could go to the wrong recipient.
- 1d81818: Messages built by `gmail +send`, `+reply`, `+reply-all` and `+forward` no longer put this computer's host name in the generated `Message-ID` (visible in `--dry-run` plans and saved drafts); the ID now ends in `@localhost`.
- 0abdb7c: Gmail helpers that take a single `--message-id` (`+read`, `+reply`, `+reply-all`, `+forward`, `+download`, `+unsubscribe`) now reject a value that is not a Gmail API message ID (for example `<ID>` or an RFC `Message-ID` header such as `<abc@mail.example.com>`) with a validation error (exit 3). Previously `+reply`, `+reply-all` and `+forward --dry-run` crashed on such a value in debug builds and printed a malformed `In-Reply-To` header in release builds. Threading headers are also checked before a message is built, so an ID containing angle brackets, white space or control characters is an error instead of a broken header.
- 8246b42: Gmail `+read`, `+reply`, `+reply-all` and `+forward` no longer treat a whitespace-only `text/plain` alternative as the message body when the message also has an HTML part: the HTML part is rendered as text instead (links kept as footnotes), so `+read` no longer prints an empty body for such messages. When a message has no text or HTML body part at all, the fallback to the API snippet (which Gmail truncates) now logs a warning on stderr.
- c6efbe5: `gmail +read` now applies `--jq`, `--format`, `--columns` and `--compact` to its JSON output instead of always printing pretty JSON.
- ec8cd76: Gmail helpers no longer download an unnamed text attachment as if it were the message body when its `Content-Disposition` starts with whitespace (` attachment`). The part keeps its attachment ID and is listed as a file, as it already was without the leading space.
- 473ebeb: `gmail +reply`, `+reply-all` and `+forward` now follow RFC 5322 §3.6.4 when the original message has an `In-Reply-To` header but no `References` header: the new `References` header starts with that parent ID, followed by the original's Message-ID. Before, the parent was dropped and the chain held only the original's Message-ID, so clients that thread by `References` lost the earlier message.
- cb189e9: `gmail +reply`, `+reply-all` and `+forward` now parse the original's `References` and `Message-ID` headers as RFC 5322 message-ID lists. `<a@x><b@x>` (no space between IDs) was read as one ID, and a `Message-ID` followed by a comment such as `<id@x> (added by relay)` produced a malformed `In-Reply-To: <<id@x> (added by relay)>`, which breaks threading in the recipients' mail clients (debug builds panicked on both).
- 34ada07: `gmail +send`, `+reply`, `+reply-all` and `+forward` now reject a `--subject`, `--to`, `--cc`, `--bcc` or `--from` value that contains a control character (CR/LF, tab, NUL and the like) or invisible/bidi Unicode, with a validation error (exit 3) before any request. Previously the subject was sent with the line break encoded into it, and control characters in addresses and display names were dropped without a word.
- f3ce50a: Gmail `+read`, `+reply`, `+reply-all` and `+forward` no longer turn a whole `ISO-2022-KR`, `HZ-GB-2312` or `ISO-2022-CN` message body into a single `�`. These charsets cannot be decoded, so the body is now decoded as UTF-8 with a warning, as for an unknown charset, which keeps its ASCII text (words, numbers, links).
- dd677a1: `gmail +reply-all` and `+reply` now understand RFC 5322 group syntax in the original message's To, Cc and Reply-To headers. Replying all to a message addressed to `undisclosed-recipients:;` (any Bcc-only message) no longer adds a bogus `<undisclosed-recipients:;>` Cc recipient, and members of a named group such as `Team: a@example.com, b@example.com;` keep their correct addresses instead of being mangled into `Team: a@example.com` and `b@example.com;`.
- fa8dce6: `gmail +reply-all` on a message you sent only to yourself now replies to you, as `+reply` does, instead of failing with "No To recipient remains". Anyone else on the original Cc stays in Cc, and `--remove` still takes precedence.
- a3041e4: `gmail +reply` and `+reply-all` now reply to the sender (From) when the original's `Reply-To` names no mailbox, for example only an empty group such as `undisclosed-recipients:;`. Previously they failed with "No To recipient remains after exclusions and --to additions".
- 735f06e: `gmail +search` and `gmail +triage` now stop on an empty `nextPageToken` instead of listing the first page again and returning duplicate messages, and fail on a repeated token instead of looping forever.
- 879316a: Gmail `+search` and `+triage` skip a message that was deleted between listing and fetching (404) with a warning on stderr, instead of failing the whole run. The other results are printed and the command exits 0, as `+watch` does for a message that is gone. Any other failed fetch still fails the command.
- e0a8d95: Gmail `+read`, `+reply`, `+reply-all` and `+forward` now decode the HTML entities in Gmail's snippet when a message has no text or HTML body and the snippet is used instead. Such messages read `Tom & Jerry's` rather than `Tom &amp; Jerry&#39;s`, and HTML replies and forwards no longer quote them double-escaped (`&amp;amp;`).
- 1140241: `gmail +read`, `+reply`, `+reply-all` and `+forward` now keep the whole body of a message whose text is split into several parts around inline attachments, as Apple Mail sends it. Only the first `text/html` (or `text/plain`) part was used, so everything after the first inline attachment was missing from the output and from the quoted original. The alternatives of `multipart/alternative` are still never joined, and the text of an embedded `message/rfc822` message or of a text part marked `Content-Disposition: attachment` is not added to the body.
- 1f8c15b: `gmail +reply`, `+reply-all` and `+forward` no longer fail with "Message is missing Message-ID header" (exit 5) when the original message has no `Message-ID`. As RFC 5322 §3.6.4 says, the new message is sent without `In-Reply-To`, and with `References` only when the original had a chain of its own. It stays in the original's Gmail thread, and a warning on stderr says that other mail clients may not thread it.
- 9970c2a: `gmail +unsubscribe --dry-run` now loads no credentials and sends nothing, as documented for `--dry-run`. Before, it loaded credentials and read the message's headers from Gmail, so it failed with an auth error (exit 2) when no account was signed in. The plan now lists the metadata read and the one-click POST, with a placeholder for the unsubscribe URL, which only the message itself can supply.
- 7c85264: `gmail +unsubscribe` now exits 10 (network error) instead of 5 (internal error) when the sender's one-click unsubscribe server can't be reached (connection refused, DNS, TLS or timeout).
- 69421e9: Gmail `+send`, `+reply`, `+reply-all`, `+forward` and `--draft` now give the message upload the same size-scaled time limit as other media uploads (the response timeout plus one second per 64 KiB). Before, a large message with attachments on a slow link could hit the plain `--timeout` (60 seconds by default) while it was still being sent. That failed the command even though Gmail might have sent the message.
- e7fcde2: Gmail `+read`, `+reply`, `+reply-all` and `+forward` decode a `charset=utf-16` message body that has no byte-order mark as big-endian, as RFC 2781 requires. It was read as little-endian and came out as unrelated CJK characters. A byte-order mark still decides the byte order, and `utf-16le` / `utf-16be` keep their own.
- 4f3a70e: When Gmail history since `gmail +watch`'s last historyId is no longer available, the command now exits with the API error (exit 1, keeping Google's reason and message, plus a note to restart `+watch`) instead of an internal error (exit 5).
- b18471b: `gmail +watch --label-ids` now emits only messages that carry one of the watched labels. Previously the label filter only decided which mailbox changes triggered a notification; every message added since the last notification was then emitted, including sent mail, drafts and messages under other labels.
- 98399bf: When `gmail +watch` setup fails after it created its Pub/Sub topic or subscription (granting publish permission, creating the subscription, or `users.watch`), it now deletes the resources it created, never a `--topic` you passed, and still returns the original error.
- f62a263: When `gmail +watch` cannot grant Gmail publish permission on the topic it created, the command now exits with the real error class (for example 1 for a refused permission, 10 for a lost connection) instead of 5, and the message still contains the `gcloud` command for granting the permission by hand.
- 83f1661: `GWSR_PROFILE` together with `GWSR_TOKEN`, `GWSR_TOKEN_FILE` or `GWSR_CREDENTIALS_FILE` is now a configuration error (exit 8), as `--profile` already was. The token or credentials file used to win silently, so the command ran as another identity than the profile you selected. A profile pinned in `config.toml` with `gwsr auth use` is still overridden by those variables.
- 2ba9d7b: `--help` and `--version` no longer print raw ANSI escape codes (`←[1m`) on a Windows console that does not render them. Help output now makes the same color decision as error messages on stderr.
- 78eb944: `gwsr help <service> ...` no longer fails with "unrecognized subcommand" when a global option such as `--format`, `--pretty` or `-v` follows the command path (`gwsr help drive files list --format table`).
- 3700cd3: Helper commands now treat an HTTP `308` that cannot be followed (one without a `Location` header) as an error, not a success. Before, a listing page answered with one ended the listing early as if it were complete. A download answered with one saved the `308` body as the file. Any other request answered with one reported success with an empty result.
- ac73516: Helper commands (`+verb`): a connection that drops or stalls while the response body is being read now exits `10` (`networkError`), as generated API methods already did. Previously the helper REST client reported it as an internal error (exit `5`).
- d9caaae: Helper `--dry-run` plans now list every query parameter the real request sends. `gmail +search --include-spam-trash` shows `includeSpamTrash`. A key the request repeats, such as `sources` in `people +find --directory`, is now shown as an array of all its values instead of only the last one.
- 230d042: `--format table` and `--format csv` now print one row per item for helper list results (`calendar +agenda`, `gmail +search`, `classroom +courses`, `tasks +list`, ...) instead of a single key/value row with every item squashed into one cell.
- 0e0606d: `gmail +watch`, `events +subscribe` and `gmail +attachments` now write `--output-dir` files through the shared output writer: a symlink at a target name is replaced instead of followed, `GWSR_RESTRICT_PATHS` is checked per file, and attachments get the default output file mode.
- 6d381d4: When a `+helper` fails for a missing OAuth scope (403 `insufficientPermissions` / `ACCESS_TOKEN_SCOPE_INSUFFICIENT`) or an auth error, the hint now names the scopes the helper requested, for the active profile: for example `gwsr auth login --scopes calendar` for `calendar +insert`, or `gwsr auth login --scopes drive.readonly,chat.messages.create` for `workflow +file-announce`. Scopes the recorded grant already holds are left out. It used to suggest `gwsr auth login -s <service>`, which without a terminal grants only read-only scopes (so write helpers kept failing) and grants nothing for `workflow`.
- 56e71d6: Helpers that write straight to stdout now end quietly with exit status 0 when the reader closes the pipe (`gwsr ... | head`), like every other command. Previously `drive +download --output -`, `drive +export --output -`, `gmail +read`, `gmail +watch` and `events +subscribe` reported a broken pipe as an internal error (exit 5) on stderr.
- 57e5ed8: `--output -` on the download helpers (`drive +download`, `drive +export`) now honors the `--timeout`/`GWSR_TIMEOUT` idle limit and checks `Content-Length`, like a download to a file. A body that stalled mid-stream used to hang forever; it now fails as a network error (exit 10), and a dropped connection mid-stream is also reported as a network error instead of a generic one.
- 3fe7efd: Tests: helper CLI tests keep `SystemRoot` and the TLS certificate variables, so tests whose mock request must succeed pass on Windows and in the Nix build.
- e4e5680: `docs +write`/`+create --text`, `docs +replace --find` and `sheets +append`/`+write --values` accept a value that starts with `-`. Before, a Markdown list (`--text '- item'`), a rule (`---`), a negative number (`--values '-5,abc'`) or a search for `-5` was rejected as an unknown flag.
- 306d76c: Helper commands (`+share`, `+upload`, `+sync`, `+agenda` and the rest) are now offered only for the API version they were written for. Before, `gwsr drive:v2 +share` or `--api-version v2` ran the Drive v3 helpers against the v2 endpoints, sending v3 field names (`emailAddress`, `sendNotificationEmail`, `name`) that v2 does not accept and reading a `files` list that v2 does not return. Another version now gets only its generated Discovery commands.
- a577d7a: `drive +upload --folder-id`, `drive +share --email`/`--domain`, `script +push/+pull/+run/+logs --script-id` and `tasks +add --title` now refuse an empty or blank value with a validation error (exit 3) before anything is sent. Before, the empty value went to the API as `parents: [""]`, `emailAddress: ""`, `domain: ""`, `scriptId=` or `title: ""`.
- 8f4bbbe: `gwsr auth status` reads the OAuth client from the config directory it reports, and unit tests no longer read or write the real config, cache, keyring or gcloud credentials.
- 1c1448e: A rejected URL (`GWSR_API_BASE_URL`, a Discovery `rootUrl`, a request URL) whose unencoded password contains `?` or `#`, such as `https://user:pa#ss@proxy.example/`, no longer repeats the password in the error message. Those characters ended the part that was hidden, so the password was shown in full.
- 4dfedf3: Error hints now name settings that work for the command that failed:
  
  - The network-error hint suggested raising `--timeout`, and the quota-project hints said to pass `--no-quota-project`. Only generated API methods have those flags, not helpers or `gwsr batch`. The hints now lead with `GWSR_TIMEOUT` / `timeout_secs` and `GWSR_NO_QUOTA_PROJECT=1`, and mention the flags as the per-method alternative.
  - The rate-limit hint suggested "lower concurrency", a setting gwsr does not have. It now says to run fewer calls in parallel, or to raise `--page-delay` / `GWSR_PAGE_DELAY_MS` when paginating.
  - The `events +subscribe` error for an operation that reports no subscription yet had a run of stray spaces in it, and named `gwsr events operations get` without the required `--params`. It now gives the full command.
- 85d4124: `gwsr auth login` honours `$BROWSER` again on Linux and other Unix systems (it stopped when the browser launcher was rewritten to withhold `GWSR_*` variables). `$BROWSER` is a `:`-separated list of commands; each is split into words without a shell, `%s` is replaced by the URL (or the URL is appended), and the first command that starts wins before falling back to `open`, `xdg-open` or `wslview`. The browser still never sees any `GWSR_*` variable. A launcher that cannot be started for a reason other than not being installed no longer stops the search for the next one.
- dfdf476: `--impersonate` now rejects values that are not email addresses (exit 3), as `GWSR_IMPERSONATE` already did, instead of passing them into the service-account assertion, `auth status` output and cache keys.
- ceb0346: Request-body validation now range-checks `int32` and `uint32` fields given as JSON numbers, as it already did for their string form: `{"count": 5000000000}` for an `int32` field is refused before the request is sent instead of being rejected by the API. Fractions such as `1.0` stay refused in both forms.
- 1f46c0a: A refresh token rejected by Google (`invalid_grant`) no longer always says "Run `gwsr auth login` to sign in again". For `GWSR_CREDENTIALS_FILE`, which takes precedence over every profile so a new login changes nothing, the error now names the file and says to replace it or unset the variable. For `GOOGLE_APPLICATION_CREDENTIALS` it names that file, and for gcloud's Application Default Credentials it suggests `gcloud auth application-default login`. `gwsr auth status` reports the same fix.
- 278de84: `gwsr commands` lists each subcommand of a helper group (`gmail +filter create`, `+filter list`, `+filter delete`) with its flags, instead of one flagless `+filter` entry that is not runnable on its own.
- 8243f63: Treat the invisible bidi marks LRM, RLM and ALM (U+200E, U+200F, U+061C), the word joiner and invisible operators (U+2060 to U+2064) and U+180E as dangerous characters: they are now rejected in arguments and paths and stripped from terminal output, like zero-width spaces and bidi overrides already were.
- ad2c5a9: A `--jq` filter whose only result is an empty string now prints an empty line, as `jq -r` does, instead of nothing. With `--page-items` or `--page-all`, an item or page whose result was `""` (a file with an empty `description`, say) was dropped from the stream, so the output had fewer lines than items.
- 61822dc: A `--jq` filter or output format that fails after a command already changed something (sent a message, trashed it, created a task, filter or Meet space, or ran a generated POST/PATCH/DELETE method) now says the change was made and names it, instead of a plain validation error that reads as "nothing happened" and invites a duplicate retry.
- c0a3b09: `--jq` results containing `nan` or an infinity no longer fail as "not valid JSON". As in jq, `nan` prints as `null`, and `infinite` / `-infinite` print as the largest finite double with the same sign (`1.7976931348623157e308` / `-1.7976931348623157e308`). This applies at any depth inside arrays and objects.
- 4e2c6a4: `--jq` no longer offers jq's `env` builtin. It returned the whole process environment, so a filter such as `--jq env.GWSR_TOKEN` printed the access token, client secret or any other secret variable to stdout, although gwsr never echoes them itself. A filter that uses `env` is now rejected before any request (exit 3).
- 0c0eb7d: `--jq` no longer offers jq's `stderr`, `debug`, `debug(msg)` and `halt_error` builtins. They wrote their input to stderr as is, bypassing gwsr's terminal sanitization, so a filter such as `--jq '.[].subject | stderr'` could send escape sequences from API data (a message subject, a file name) straight to the terminal. A filter that uses them is now rejected before any request (exit 3). `input` and `inputs` remain unavailable.
- d91407a: Generated commands now recognize a JSON response whatever the letter case of its `Content-Type` (`Application/JSON; charset=UTF-8`, `TEXT/JSON`), as media types are case-insensitive. Such a response used to be refused as "binary" and needed `-o` to read.
- 1b92f64: Floating-point numbers in API responses are now printed exactly as the API sent them. Before, many 17-significant-digit doubles, such as coordinates or unformatted Sheets values, were read slightly wrong and printed with a changed last digit (for example `13.346133595589677` became `13.346133595589675`) in every output format.
- 425ba6b: JSON read from a file or stdin (`--params`/`--json` with `@file` or `-`, and `gwsr batch` input) may now start with a UTF-8 byte order mark, as files saved by Windows editors and Windows PowerShell do. Previously such input was rejected with `expected value at line 1 column 1` (exit 3).
- c809366: Generated commands parse an `application/json+protobuf` response (Google's JSON encoding of protobuf messages) as JSON again, in any letter case. Since the exact JSON content-type check it was refused as binary and needed `-o`. Other `+json` types, such as a Drive `application/vnd.google-apps.script+json` export, are still returned as text.
- 3f6ffd9: `keep +list --changed-since` keeps the fractional seconds of the given time. They were dropped, which moved the bound earlier and returned notes changed before the requested time; passing a note's own `updateTime` returned that same note again.
- 59e9987: `--format csv` and `--format table` now render the items of People `connections.list`, Gmail `history.list` and Drive `changes.list` responses as rows. Their list metadata (`totalPeople`, `historyId`, `newStartPageToken`) made the formatter treat the whole response as a single object, so the items were printed as one cell of JSON blobs.
- 9e8a6cd: `GWSR_LOG_FILE` / `log_file`: an existing log directory is now restricted to `0700`, as documented, instead of keeping looser permissions such as `0755`. A directory whose permissions cannot be set is an error.
- 01fc410: `gwsr auth login`: when Google rejects the authorization code (`invalid_grant`, for example a code that expired or was already used), the error now says so. It used to claim that a stored refresh token had expired or been revoked. This error, the "no refresh token" error and a refused-scope error now name the profile being signed in to (`gwsr auth login --profile work`) instead of the default profile.
- 3628806: `gwsr auth login` to the default profile no longer writes `profile = "default"` to `config.toml`. Writing it made the default profile count as explicitly chosen, so after `gwsr auth logout` every command failed with "profile 'default' has no credentials" instead of falling back to Application Default Credentials (`GOOGLE_APPLICATION_CREDENTIALS` or gcloud's ADC file), and every re-login hint added `--profile default`. A first login to a named profile still makes it the active one. An existing `profile = "default"` line written by an earlier version can be removed by hand.
- ef65e17: `gwsr auth login --scopes email,profile` now requests these OpenID aliases under the names Google reports them as granted (`userinfo.email`, `userinfo.profile`). Login no longer warns that `email` and `profile` "were not granted" when they were, and `requested_scopes` no longer lists each identity scope twice.
- 776060e: `gwsr auth login --profile NAME` no longer makes `NAME` the active profile when the default profile is already signed in. Since default-profile logins stopped writing `profile = "default"` to `config.toml`, the first login to any named profile took over as active. Every later command without `--profile` then quietly ran as that other account. A named login now becomes active only when nothing is configured and the default profile has no credentials. Use `gwsr auth use NAME` to switch.
- 1e1b60e: `gwsr auth login --scopes` now rejects an entry that contains whitespace, control or non-ASCII characters (for example `--scopes "gmail.modify drive"`) with a validation error asking for comma-separated scopes. Before, such an entry was recorded as one bogus scope, while Google read it as several, so the "not granted" warning and the saved scope list were wrong.
- bb0875d: `gwsr auth login -s events` now requests the scopes the `events +subscribe` and `events +renew` helpers use (Chat messages, reactions, memberships and spaces, Meet spaces and Drive read scopes, plus Pub/Sub with `--write` or `--full`). It used to request only `drive.readonly`, because the Drive preset made `events` look covered. `-s workflow` (and `-s wf`) now requests the workflow helpers' scopes instead of failing to look up a Discovery document that does not exist. Neither service can make another one in the same `-s` list (such as `chat`) look covered any more.
- 6889dcb: `gwsr auth login` now requests exactly the scopes you chose:
  
  - `--services` no longer keeps `cloud-platform` (for example with `--full -s gmail`), and the scope picker no longer lists it as belonging to every service. Request it with `--scopes cloud-platform` or plain `--full` if you need it.
  - The picker's "Full access" preset no longer adds `cloud-platform` when it is not a listed, checked row.
  - When `--services` names a service the picker has no scopes for (for example `-s chat` without `gwsr auth setup`), the picker is skipped and that service's scopes come from its Discovery document instead of being dropped.
  - An unknown `--services` name (such as `calendar.events`), or a service whose scopes cannot be looked up, is now an error instead of a warning.
- 6530671: `gwsr auth login` now rejects `-s/--services` combined with `--scopes` (exit 3) instead of silently ignoring `-s`. `-s` filters the presets (`--write`, `--full`, the read-only default and the picker); `--scopes` is an exact list. To add scopes for more services, run another `gwsr auth login` (logins are incremental).
- c99cb09: `gwsr auth login` now exits 2 (`authError`) when the browser sign-in times out, instead of 8 (`configError`). A timed-out sign-in leaves the profile without credentials; nothing about the configuration is wrong.
- 4662e71: `gwsr dev man` writes each page atomically and replaces a symlink at a page's path instead of writing through it, so a symlink in `--output-dir` can no longer overwrite the file it points to (or, with `GWSR_RESTRICT_PATHS=cwd`, reach outside the current directory: that is now refused). Every page is rendered before any is written.
- 733a904: The `gmail +watch` and `events +subscribe` help tips, their skills and the README now describe `--max-failures N` correctly: the stream tolerates N consecutive transient failures and exits on the next one. They used to say it exits after N failures.
- 268b707: `alt=media` downloads with `-o PATH` or `-o -` now deliver the file's exact bytes even when it is JSON. Previously a stored JSON file was parsed and re-serialized (keys reordered, numbers and whitespace rewritten). A JSON file with `name` and `metadata` keys was polled as a long-running operation, and a file that was not one valid JSON document (for example NDJSON) failed to download.
- a7fa4fa: A method whose Discovery document sets `useMediaDownloadService` (such as Artifact Registry `files download`, Cloud Support `media download` or Drive `files get`) now sends an `alt=media` request to the download service URL (`<rootUrl>download/<servicePath>…`), as Google's generated client libraries do. Requests without `alt=media` keep the regular URL.
- 5852947: `gwsr workflow +meeting-prep --dry-run` now lists the query parameters the real run sends (`timeMin`, `singleEvents`, `orderBy`, `maxResults`) instead of an empty query.
- ab66bc7: `workflow +meeting-prep` now shows the next meeting that has not started yet. It used to return the first event whose end was after now, so an all-day event (such as a working-location or holiday entry), a meeting already in progress, or an event you declined was reported as your next meeting.
- d34ca3e: `--sanitize` no longer treats a Model Armor result whose filters did not all run as clean. A `NO_MATCH_FOUND` verdict now counts only with `invocationResult: SUCCESS`; `PARTIAL` or `FAILURE` invocations, and an unspecified or unknown `filterMatchState`, are sanitization failures, so `block` mode withholds the output (and sends nothing for the Gmail send helpers) and `warn` mode annotates it with `_sanitization.error`. A `MATCH_FOUND` is still a match even when some filters were skipped.
- 2c97e13: `modelarmor +create-template`, `+sanitize-prompt` and `+sanitize-response` now require `--json` to be a JSON object and reject repeated keys, as every other `--json` body does (exit 3). Before, `--json '[]'` or `--json '"x"'` was sent as the request body, and `{"a":1,"a":2}` silently sent only the last value.
- 1426e44: `modelarmor +sanitize-prompt` and `+sanitize-response` now retry a timeout or a 5xx response like any other idempotent call. They used to fail at once, although sanitizing only screens text and changes nothing on the server. The `--sanitize` path already retried. `+create-template` is still never re-sent once the request may have reached the server.
- ef6636d: `modelarmor +sanitize-prompt` and `+sanitize-response` now exit 3 (validation error) instead of 5 (internal error) when the text piped on stdin can't be read, for example when it isn't valid UTF-8, matching every other flag that reads stdin.
- 850ca0f: A multipart `--upload` (files up to 5 MiB) of a file that grows or shrinks while it is being sent now fails with a validation error (exit 3) naming the file, and nothing is uploaded. Previously a file that grew was cut off at the originally measured length, so the server received the new bytes with a truncated closing boundary, and a file that shrank failed with an opaque network error.
- 29c8e6b: An empty `NO_COLOR=` no longer turns color off. Following no-color.org, `NO_COLOR` disables color only when it is set to a non-empty value. This applies to stderr messages and to `--help` output.
- 8902ef4: npm launcher: `SIGTERM`, `SIGINT`, `SIGHUP` and `SIGQUIT` sent to the `gwsr` launcher are now forwarded to the native binary, and the launcher waits for its real exit status. Previously the launcher died on its own and left `gwsr` running as an orphan (for example under `docker stop`, Kubernetes or `kill <pid>`), which skipped the clean shutdown of `+watch`/`+subscribe` streams. When the binary dies from a signal Node cannot re-raise (such as `SIGPIPE`), the launcher now exits with `128 + signal number` instead of `1`.
- 5d5ebc9: The npm launcher now installs its signal-forwarding handlers before it starts `gwsr`, so a SIGINT/SIGTERM/SIGHUP/SIGQUIT that arrives just after the binary starts is forwarded to it instead of killing the launcher and orphaning the binary.
- 98df5a8: npm: the `gws-rust` package and every platform package (`gws-rust-<os>-<cpu>`) now include the `NOTICE` file, as the Apache-2.0 license requires for redistribution. Before, the package build copied `NOTICE` into the main package but its `files` list left it out of the published tarball, and the platform packages, which carry the binary, never had it.
- be46fbe: Tests: the shared OAuth HTTP client no longer reuses idle connections in unit tests, fixing intermittent "error sending request" failures in token tests.
- cbcedc1: `gwsr auth login` no longer stalls for about 10 seconds after you approve access in the browser. The local callback listener now serves connections concurrently (up to 16 at once), so an idle connection the browser opens ahead of time can no longer hold up the real redirect.
- 602e401: An `--output-dir` (or `+sync --dir`) that names an existing file is now refused up front with a validation error (exit 3), instead of failing with an internal error (exit 5) part-way through the command.
- 9d832de: `gmail +watch` and `events +subscribe` now refuse `--jq`, `--columns` and a non-JSON `--format` together with `--output-dir` (exit 3). Before, these flags were silently ignored: `--output-dir` writes each record as JSON to its own file and prints nothing on stdout. A `format` default from the environment or `config.toml` is not affected.
- 58a639d: Helper `--output PATH` (`drive +download`/`+export`, `docs +read`, `sheets +read`, `forms +responses`) refuses an existing file without `--overwrite` before loading credentials or sending any request (exit 3). Before, the refusal came only after the content was fetched, and without credentials the command failed with an auth error (exit 2) instead. `drive +download`/`+export` without `--output` also check the name taken from Drive before starting the download.
- b02e8e9: `-o/--output` naming an existing directory is now rejected before any request is sent, with a validation error (exit 3). Previously `gwsr` downloaded the whole response and then failed with an internal error (exit 5) when it tried to rename the file over the directory, or, for helpers without `--overwrite`, asked you to pass `--overwrite`, which could not help.
- 107f404: `-o/--output` refuses a path that ends in a separator, `.` or `..` (`-o downloads/`, `-o downloads/.`) before any request, like an existing directory (exit 3). When the directory did not exist yet, the trailing separator was dropped and the payload was saved as a file named `downloads`.
- fc6fc12: With `--page-all`/`--page-items`, a later page that comes back empty (204 or no body) now ends the stream with a warning on stderr instead of printing a `{"status":"success"}` object into the NDJSON output.
- 072fc83: `--page-all` adds the top-level `nextPageToken` to a `fields` mask that only names it inside a sub-selection (for example `files(id,nextPageToken)`), instead of stopping silently after the first page.
- 7e32901: `--page-all` / `--page-items`: a later page that comes back as something other than JSON (for example an HTML proxy or captive-portal page served with status 200) now fails the run with exit `5` and names the `pageToken` to resume from. Previously the page was wrapped as a `{"status":"success",...}` object, mixed into the NDJSON stream, and the truncated listing exited `0` as if it were complete.
- bb460e3: `--page-all` (and `--page-items`) no longer loops forever when an API returns a `nextPageToken` it has already returned. The pages fetched so far are still printed, then the command fails with an error that names the repeated token, instead of sending requests without end under the default unlimited `--page-limit`.
- 713dbbf: `--page-all` resumed from a `pageToken` no longer prints that page twice when the API hands the same token back; the repeat is reported after the first page.
- bb53336: `--page-items` now detects the list field of Calendar `events list` (and other responses with list-metadata arrays such as `defaultReminders`) instead of failing with "several array fields".
- 2b5b3fd: `--columns` with `--page-items` or `gwsr batch` no longer fails when the first item lacks a selected field: that cell is empty, and only a column no item has is an error, reported after the rows.
- 99b9de2: `--page-items --format csv|table` (and `gwsr batch` with those formats) now print every item as one row. An item shaped like a list envelope, such as a Drive file selected with `--fields 'files(kind,parents)'`, was split into rows of its own array field and lost its other fields.
- 5a38d1c: `--page-items` with `--sanitize` in `warn` mode no longer drops the `_sanitization` annotation. Model Armor screens each page, and the page's annotation (a match, or a sanitization failure) is now attached to every item printed from that page. Before, the annotation was discarded with the page envelope, so the printed items looked screened and clean.
- 9355745: `--page-items` now skips a deprecated list field that sits next to its replacement, so Chat `spaces search` splits `results` into rows instead of failing with "several array fields (results, spaces)".
- 2399041: A bare `--page-items` now picks `usageReports` for Admin Reports usage methods. It used to fail with "several array fields (usageReports, warnings)" because it treated the report's `warnings` array as a second list. `--page-items=warnings` still selects the warnings explicitly.
- 594c6f9: `--page-items=FIELD` now fails with a validation error (exit 3) that lists the response's list fields when `FIELD` appears neither in the response schema nor in the page. Before this fix, a typo such as `--page-items=file` for `drive files list` printed nothing and exited 0.
- 41b25f3: `--page-limit` and `--page-delay` on a generated list method now fail with a validation error (exit 3) unless `--page-all` or `--page-items` is also given. Before, they were silently ignored and only the first page was fetched, so `--page-limit 5` alone returned one page. `page_limit` / `page_delay_ms` set in `config.toml` or the environment still apply only to paged runs and are not rejected.
- 4381cfe: `--page-all --format csv|table` no longer turns a page without items (Google omits empty lists) into a row of `kind`/`nextPageToken` metadata, which on the first page fixed the header to those fields and dropped every real item that followed.
- 36809c4: `workflow +meeting-prep`, `events +renew --all` and `gmail +watch` history paging now share one page-token rule: an empty `nextPageToken` ends pagination, and any token the API repeats is an error instead of an endless loop.
- 149a1cf: A number or boolean given for a parameter with Discovery `enum` values (for example `"format": 7`) is now checked against those values instead of being sent unchecked.
- b418dc9: `--params`, `--json` and `gwsr batch` input lines now reject JSON objects that repeat a key, such as `{"fileId":"a","fileId":"b"}`, with a validation error (exit `3`) that names the key. Previously the last value silently won, so an ambiguous argument could target a different resource than intended, for example in a delete.
- 04b365e: `--page-all` (and `--page-items`) now adds `nextPageToken` to a `fields` mask passed in `--params`, the same way `--fields` already did. Before this change, `--params '{"fields":"files(id)"}' --page-all` stopped after the first page because the API left out the token, and the command still exited 0.
- b192c27: Refuse requests whose URL path would contain a `.` or `..` segment (an ID or resource name of `.` or `..`, literal or percent-encoded) with a validation error (exit 3), in `--dry-run` too. URL parsing removed such segments, so the request addressed a different resource: `gwsr calendar +delete --calendar-id CAL --event-id ..` sent `DELETE …/calendars/CAL/` (the calendar, not an event), and `drive permissions delete` with `"permissionId": ".."` sent `DELETE …/files/FILE_ID/`.
- bd1b655: `gws-rust-core` path resolvers (`resolve_file_path`, `resolve_output_dir`, `resolve_dir_path`, `check_derived_path`) now resolve a relative `cwd` against the process working directory. Before, a leading `..` was dropped lexically, so `../x` with a `cwd` of `.` resolved to `x` (the wrong file) and the result was not absolute.
- 534b99b: `gwsr people +find` now rejects an empty `--query` or a contact-search `--limit` above 30 before loading credentials, so it exits with a validation error (3) instead of an authentication error (2).
- e86090e: Profile names must now be lowercase (`[a-z0-9_.-]`). On case-insensitive file systems (macOS and Windows by default) `Work` and `work` were the same profile directory, so `gwsr auth login --profile work` silently replaced the credentials of profile `Work`, `gwsr auth use work` switched to them, and `gwsr auth list` showed no active profile. **Breaking:** a profile with uppercase letters in its name is now rejected; log in again under a lowercase name.
- e3624d7: Profile names can no longer end with `.` or be a Windows device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`, with or without an extension). Windows drops trailing dots from directory names, so `gwsr auth login --profile work.` wrote to the `work` profile's directory and replaced its credentials, and a device name opened the device instead of a profile directory. **Breaking:** such a name is now rejected; log in again under another name.
- 74d11fe: When `gmail +watch --cleanup` or `events +subscribe --cleanup` cannot delete the Pub/Sub resources it created, the command now exits with the class of the failure (for example 1 for a refused permission) instead of 5, and the error still names every resource it could not delete.
- 687d776: `events +subscribe` and `gmail +watch` exit with the real error class when creating, authorizing or deleting Pub/Sub resources fails: a network failure is exit 10 instead of 5, a refused token is exit 2 instead of 5, and a 403 rate limit stays retryable (exit 6) instead of becoming exit 1.
- bd51617: `gmail +watch` and `events +subscribe` now treat a Pub/Sub pull whose response body stops arriving for the pull timeout as "no messages yet", the same as a pull whose headers never arrive. Before, it was a network error that counted against `--max-failures`.
- 07bc5ad: `events +subscribe` and `gmail +watch` no longer treat a Pub/Sub pull whose connection timed out as "no messages yet". A connect timeout means the request was never sent, so it now counts as a transient network failure (bounded by the stream's failure budget) instead of being silently retried forever.
- b435d36: `-q` now silences the info and warning diagnostics of `gmail +watch` and `events +subscribe` and the upload progress bar, so it prints only errors on stderr as documented.
- 2cac464: A 403 caused by the quota project sent as `x-goog-user-project` (the caller lacks `serviceusage.services.use` on it, for example because the OAuth client belongs to a project the user is not a member of) now carries a hint pointing at `--no-quota-project` / `GWSR_NO_QUOTA_PROJECT=1` and `GWSR_PROJECT_ID`.
- f30f257: A 403 whose error body names a rate-limit reason after another reason (a later `errors[]` or `details[]` entry) now exits 6 (retryable) with that rate-limit reason, matching the client, which already retried it; it exited 1.
- dcfcf66: `GWSR_REQUIRE_CONFIRM=1` now also gates the generated methods that send mail or messages, share files or run scripts (`gmail users messages send`, `gmail users drafts send`, `chat spaces messages create`, `drive permissions create`/`update`, `script scripts run`, `admin members insert`), share a calendar (`calendar acl insert`/`update`/`patch`), give others access to or route mail out of a mailbox (`gmail users settings delegates create`, `updateAutoForwarding`, `forwardingAddresses create`, `sendAs create`, `filters create`), or email event attendees (`calendar events insert`/`update`/`patch`/`move`/`quickAdd` with `sendUpdates` `all`/`externalOnly` or `sendNotifications=true`; without those parameters Calendar sends no notifications, so the call is not gated), both directly and inside `gwsr batch`. Previously only the `+` helpers were gated, so the policy could be bypassed by calling the raw method. These methods now accept `-y/--yes`.
- 30908bd: Recipe skills: Sheets ranges containing `!` are single-quoted (double quotes let interactive bash history-expand them), `docs +write` steps that write Markdown headings pass `--markdown` (they inserted literal `##` text), multi-line `docs +write` content is piped through `printf` so each step stays one line, and the weekly-schedule recipe uses `calendar +agenda --week` (plain `+agenda` shows one day).
- 5e4b788: A redirect from a Google API to another origin no longer carries the quota project or the request body. The `Authorization` header was already dropped, but `x-goog-user-project` and every other request header still went to the new host, and a 307/308 also re-sent the method and body there. Such a redirect is now followed as a plain GET (or HEAD) that keeps only `Accept`, `Accept-Language`, `Range` and `User-Agent`. A redirect that would keep another method, such as a 307/308 after a POST or PUT, is refused with a network error (exit 10) instead of sending the body to a host it was not meant for. Redirects within one origin behave as before.
- 97e5272: Tests only: the recipe and persona command check validates `gwsr` commands that a step pipes input into (`printf ... | gwsr docs +write --text-file -`) again. Since #442 these steps began with `printf`, so they were skipped. No behavior change.
- cb2646b: An empty ID in front of a custom method is now refused before anything is sent, real run and `--dry-run` alike. `docs +write --document-id ""`, `script +run --script-id ""` and `sheets +clear --range ""` used to send (or plan) `POST .../documents/:batchUpdate`, `.../scripts/:run` and `.../values/:clear` and fail at Google with an HTML 400 page; they now exit 3 with an "empty segment" validation error, like an empty ID anywhere else in the path.
- b878a6b: Helpers and workflows refuse an empty ID instead of sending a request to the wrong resource. Before, `calendar +delete --event-id ""` sent `DELETE .../events/` and reported `"deleted": true`, and `workflow +email-to-task --message-id ""` fetched `.../messages/` rather than one message. Any request URL whose path has an empty segment (`calendars//events`, or a trailing `/`) is now a validation error (exit 3), in `--dry-run` too, and nothing is sent.
- 4237657: An empty path parameter, such as `--params '{"fileId":""}'`, is now a validation error (exit 3). Before this change it shortened the URL to the parent collection: `drive files delete` sent `DELETE .../drive/v3/files/`, and `gmail users messages get` requested `.../users/me/messages/`.
- 1f8fd14: Release notes for a prerelease no longer tell you to `brew install`. The Homebrew tap only tracks stable releases, so that command would have installed the previous stable version.
- af1ae3c: `events +renew --all` now exits `6` (retry later) when every subscription it failed to renew failed with a retryable error (429, 503, a 403 rate limit, ...), the same class a single `--subscription-id` renewal gets. It used to exit `1` even then, so a cron job had no signal that running again would help. When any failure is permanent (a 404, say), the run still exits `1`. Either way the error's reason is `renewPartialFailure` and the report lists every failure.
- fe557b9: A global option given both before and after a command or subcommand name (`gwsr --profile a auth --profile b list`, `gwsr --format json drive files list --format table`) is now rejected with "--profile was given more than once" (exit 3), like a repeat in one place. Previously the later value silently replaced the earlier one. Counted (`-v`) and list (`--columns`) options may still repeat.
- 71b5c4d: `gmail +reply-all --remove` now checks its addresses the way `--to`, `--cc` and `--bcc` do. Control characters (CR/LF included) and invisible or bidi Unicode in the value are an error (exit 3); before, they were silently stripped, or the address matched no recipient.
- 45414bb: `gmail +reply` to a message you sent now goes to that message's original To recipients, as in Gmail, instead of back to yourself. A message counts as yours when its From matches your primary address or the `--from` alias, and Reply-To is ignored in that case. Replying to a note you sent only to yourself still goes to you. `+reply` now also reads your Gmail profile address to detect this.
- ad1cbe8: `gmail +reply` and `+reply-all` now treat every send-as alias as you, not just the primary address and `--from`. `+reply-all` no longer copies your own alias on a reply to a message you received at that alias. A reply to a message you sent from an alias now goes to its original recipients instead of back to you. Both commands now also read the account's send-as settings when `--from` is not given.
- cd4d817: A required repeated parameter given as an empty array (for example `"permissions": []`) is now rejected instead of sending the request without it.
- 122cca4: Generated commands now check `{+name}`-style path parameters against the Discovery `pattern` before sending anything. These parameters keep `/` in the URL, so a value like `groups/g1/memberships/m1` passed to `gwsr cloudidentity groups delete` used to send `DELETE v1/groups/g1/memberships/m1` and delete a membership instead of a group. Such values are now rejected with a validation error (exit 3) that names the expected pattern. `gwsr schema` also shows each parameter's `pattern`.
- 10ca555: `{+name}` path parameters now accept documented resource names that contain `@`, `:` or other RFC 3986 path characters, such as Chat's `users/user@example.com/...` and `customEmojis/:emoji-name:`. These characters are percent-encoded as needed; `%`, `?`, `#`, whitespace and `.`/`..` segments are still refused.
- a96053f: `GWSR_RESTRICT_PATHS=cwd` now also confines `dev man --output-dir` and `auth export --unmasked --output`, which could previously write outside the current directory.
- 52d1425: `GWSR_RESTRICT_PATHS=cwd` now also confines files written *under* an output directory. `script +pull` and `drive +sync` validated `--dir` itself, but the files they created below it (named after remote files and folders) could pass through a symlinked sub-directory and land outside the current directory. Every output file is now checked before it is written.
- 5cd679c: `GWSR_RESTRICT_PATHS=cwd` no longer accepts a path such as `link/missing/../file` where `link` is a symlink pointing outside the current directory. `..` is now applied after symlinks in the existing prefix are resolved, so these paths are rejected (and, without the restriction, resolve to the symlink's real target).
- 763b8ea: Resumable uploads (`--upload` above 5 MiB or with `--upload-resumable`, and `drive +upload`) no longer loop forever when the server keeps answering `308` without persisting anything: such responses now count as failed attempts and the upload gives up after the usual retries. A `Range` reaching past the end of the file is now a clear error instead of a crash, and a `308` that acknowledges every byte is finished with a valid `bytes */<size>` request instead of a malformed `Content-Range`.
- 0c81837: A resumable upload chunk answered with a Google rate-limit 403 (`rateLimitExceeded`/`userRateLimitExceeded`) is now retried like a 429, instead of failing the upload on the first answer.
- b69e8b4: Resumable upload chunk retries now honor `Retry-After`: they wait as long as the server asks, and stop at once (exit 6) when it asks for more than 60 seconds, instead of re-sending the chunk on gwsr's own short backoff.
- 35be22d: Resumable uploads now fail instead of uploading a truncated copy when the file grows (or shrinks) while it is being uploaded.
- 7c1b0ce: Resumable uploads (`--upload` over 5 MiB or `--upload-resumable`, and helper uploads) that give up after their chunk retries now exit with the class of the last failure: a retryable status such as `503` or `429` exits `6` with the API's error message, and a lost connection or timeout exits `10`. Previously every exhausted upload exited `5` (internal error).
- 201bde2: Resumable uploads now fail when the upload server's `308` reply, or its answer to a status query, claims more bytes than `gwsr` has sent. Before, `gwsr` skipped ahead to that offset and could report a finished upload while part of the file was never sent.
- f393ffe: A resumable upload whose session has expired (`404` or `410` on a chunk) now exits `1` (API error) with Google's error message and a hint to rerun the command to start a new upload. Previously it exited `5` (internal error).
- 5dce27b: A `Retry-After` with more seconds than fit in 64 bits now counts as a very long wait, so gwsr stops retrying and returns the response, instead of ignoring the header and retrying on its own short backoff; a signed value such as `+5` is ignored as invalid.
- ec7a4db: API errors no longer carry a misleading retry note. When an earlier attempt was retried (for example a 503 with `Retry-After`) and the final answer was a non-retryable error such as a 404, the error message said "gave up after N attempts; server asked to retry after Ns". The note now appears only when `gwsr` actually stopped retrying a retryable failure, and the `Retry-After` it quotes is the final response's own.
- a129b2e: Idempotent requests are now retried when the connection is reset or closed after the request was sent, as documented for connection errors, instead of failing on the first attempt.
- ccace95: An API error whose `Retry-After` asks for more than a day no longer quotes a meaningless number of seconds. A header too large to represent used to print as "server asked to retry after 18446744073709551615s"; the note now says "server asked to retry after an unreasonably long wait (over 86400s)". Waits of up to a day are still printed in seconds.
- 4a77d7a: Exit code 6 (retryable) now covers exactly the statuses gwsr itself retries: a 408 Request Timeout exits 6 (was 1), and a 501 Not Implemented or other non-transient 5xx exits 1 (was 6).
- d8ae3fc: `calendar +rsvp --dry-run` now shows the PATCH body a real run sends: the event's whole guest list with only your entry changed, and a `comment` only when `--comment` is given. The plan used to list your entry alone with `"comment": null`, which describes a request that would remove every other guest and clear your existing comment.
- 287bef4: Update rustls to 0.23.46 to pick up the fixes for RUSTSEC-2026-0346, RUSTSEC-2026-0348 and RUSTSEC-2026-0349.
- 4355aa6: Service accounts: the signed JWT assertion's `iat` is now backdated by 10 seconds (with `exp` one hour after it), as Google's Go OAuth library does. A local clock running a few seconds ahead of Google's no longer gets the assertion rejected as issued in the future.
- 18944d1: An access token whose `expires_in` is absurdly large (up to 2^63-1 seconds) or negative no longer overflows the expiry computation. Debug builds used to panic, and a release build treated a service-account token with such a lifetime as already expired, so it was never reused from the token cache. The lifetime is now clamped, for both user refresh tokens and service accounts.
- a3f610e: `drive +download`, `drive +export` and `drive +sync` no longer fail with "File name too long" on Drive files with long non-ASCII names (emoji, CJK, accented text). Local names derived from Drive names are now capped at 200 bytes rather than 200 characters; previously one such file aborted the whole `+sync`, leaving the remaining files undownloaded.
- d0f36b6: Downloaded file names (`drive +download`, `drive +export`, `drive +sync`, `gmail +download`) can no longer become a Windows device name such as `CON` or `nul.pdf` when a very long name is shortened: the device-name check now runs on the shortened name, so the file gets a `_` prefix instead of opening the device on Windows.
- 8e979a6: Local file names derived from remote names (`drive +download`, `drive +sync` and `gmail +attachments`) are no longer left hidden or with a trailing dot when dots and whitespace are mixed at either end. A remote name such as `" .bashrc"` used to be saved as the hidden file `.bashrc`, and `"report.<no-break space>."` as `report.`, a name Windows silently changes. Dots and whitespace are now trimmed together.
- 7ef6c02: Files named after remote data (`gmail +attachments`, `drive +download`, `drive +sync`) no longer use Windows device names such as `CON.pdf`, `nul` or `COM1.txt`: they get a `_` prefix (`_CON.pdf`) on every platform, so the name is the same everywhere and Windows writes a real file instead of the device. Trailing dots and spaces, which Windows drops, are stripped. `script +pull` refuses an Apps Script file whose name is a Windows device name, since renaming it on disk would break `+push`.
- 5ffd291: When `--sanitize` withholds the output of a command that already changed something (for example `admin +user-create`), the error now says the change was made and names the resource's IDs, without showing the withheld content.
- a656197: `--sanitize` no longer lets unscreened content reach stdout. `-o -` on a generated method (an `alt=media` download, an export or any other raw payload) and `--output -` on `drive +download` and `drive +export` used to stream the raw bytes to stdout without Model Armor screening and exit 0, even in `block` mode. With `--sanitize` set, in either mode, these combinations are now refused with a validation error (exit 3) before any request is sent. Writing to a file (`-o PATH` / `--output PATH`) is unchanged.
- 66e1b35: When `--sanitize` withholds the output of `workflow +email-to-task`, `workflow +file-announce`, `gmail +label`, `+archive`, `+trash` or `+filter delete`, the error now says that the request succeeded and the change was made, and names the created task or Chat message. Before, it read like a refusal, so a retry created a duplicate task or posted the announcement twice.
- b3153c6: With `--sanitize` set, a JSON response that fails to parse no longer prints its first 200 bytes, unscreened, in the error on stderr. In either mode the error now says that the content was withheld. Without `--sanitize`, the error still quotes the first bytes.
- 52dc8c7: `gwsr schema` now honors the global `--api-version` flag, which wins over a `:version` suffix in the path, as it does when the method runs. Previously the flag was accepted but ignored, so `gwsr schema drive.files.list --api-version v2` printed the v3 schema.
- ff9726a: `gwsr schema <service>.<resource>.<method>` now lists the parameters that every method of the API accepts in `--params` (`fields`, `quotaUser`, `prettyPrint`, ...) under a new `apiParameters` key. Before, schema showed only the method's own parameters, although a rejected `--params` points to it for the accepted ones.
- b5ea55e: `gwsr schema workflow...` and `gwsr batch workflow` now fail at once with a validation error (exit 3) that says `workflow` has no Discovery document and points to `gwsr workflow --help`. They used to ask the network for a document that does not exist and failed with a misleading Discovery error (exit 4).
- 1db1b0f: `gwsr schema` keeps a dotted API version whole (`gwsr schema dfareporting:v3.4.ads.list`) instead of splitting it into a version `v3` and a resource `4`, so the schema path printed in method help works for such APIs.
- a7f12c1: Method `--help` ("Full request/response schema: gwsr schema …") and the `Invalid --params` error now name a `gwsr schema` path that works. They used to print the Discovery method id, which fails for `events` (`workspaceevents.…`), `groupssettings` (`groupsSettings.…`), `alertcenter` / `cloudsearch` methods whose id skips a resource (`alertcenter.getSettings`), and every `<api>:<version>` service (`youtube.videos.list` instead of `youtube:v3.videos.list`).
- d8d5190: The `--help` of a generated method that has no parameters now ends with its `Full request/response schema: gwsr schema <path>` line, as every other method's help does. These are mostly methods that take only a request body, such as `classroom courses create`, `sheets spreadsheets create` and `driveactivity activity query`, where the schema is the only description of what `--json` accepts.
- 4d91154: `gwsr schema` now shows the full structure of request and response bodies: the fields of inline nested objects (for example Drive `File.contentHints.thumbnail`), map values (`additionalProperties`, as in `File.appProperties`), items of nested arrays (Sheets `ValueRange.values`), and property enums and defaults. Previously these were printed as a bare `"type": "object"` or `"type": "array"`, although `--json` bodies are validated against them. `--resolve-refs` also follows references inside those nested objects.
- ac584b3: `gwsr schema <method> --resolve-refs` no longer crashes with a stack overflow on recursive schemas. For example, `gmail.users.messages.get` (`MessagePart.parts`), `docs.documents.get` (`Tab.childTabs`) and 52 other Workspace methods used to abort. A reference back to a schema that is already being expanded is now left as `{"$ref": ...}`.
- 88cead9: Auth and missing-scope hints for Chat methods no longer tell you to sign in with an app-only scope such as `chat.bot` or `chat.import`, which Google refuses in a user sign-in. They now name the first scope a user can grant, such as `chat.messages`.
- afe9087: Scope and sign-in hints now name the active profile. With `--profile work` (or `GWSR_PROFILE`, or a non-default profile), the `hint` of an auth error or a missing-scope 403, the "refused the requested scopes" error and the scope warning suggest `gwsr auth login --profile work --scopes ...` instead of plain `gwsr auth login --scopes ...`, which signed in to the config/default profile and could replace that profile's login.
- 2e829bf: `script +pull` refuses a project whose file names differ only in case (`Code` and `code`, or `lib/a` and `Lib/b`), and writes nothing. On a case-insensitive disk (the macOS and Windows default), one file used to replace the other while both were reported as written, even with `--overwrite`. Rename one of them in the Apps Script editor.
- 771dd8a: `script +pull` keeps dots in Apps Script file names: `config.dev` is now saved as `config.dev.gs` instead of `config.gs`. Before, two files such as `config.dev` and `config.prod` were written to the same local file (the second overwrote the first with `--overwrite`, or failed after a partial write without it), and `+push` renamed them on the server.
- 4f797dc: `script +pull` refuses, before writing anything, a project in which two files need the same local path: one as a file and one as a directory (`a`, saved as `a.gs`, and `a.gs/b`), or one name listed twice. Before, it wrote the files up to the clash and then failed part-way, or silently kept only the last of two duplicates.
- bd2f48b: When `script +pull` fails while writing files, the error now says how many files were written and lists the ones already on disk.
- 0fc4f53: `script +pull` now refuses a project file whose `source` is present but not a string (null, number, object, array). It names the file and writes nothing. Previously it wrote an empty local file, so a later `+push` (which replaces the whole project) could wipe that file. A file with no `source` at all is still pulled as an empty file, because the API omits empty strings.
- 8e5e0f6: `script +pull` refuses project files that `script +push` would skip: names with a part starting with `.` (such as `.config/settings`) or a `node_modules` directory. Before, `+pull` wrote them, the next `+push` of the same directory silently left them out, and because `+push` replaces every file in the project, those files were deleted from the project. Nothing is written when the project has such a file; rename it in the Apps Script editor.
- fd0a3d7: `script +pull` refuses Apps Script file names containing characters that are invalid in Windows file names (`<>:"|?*`); on Windows a name such as `C:evil` was a drive-relative path that wrote outside `--dir`.
- e6aadb1: `script +push` takes only `appsscript.json` as the manifest. A local `appsscript.gs`, `appsscript.js` or `appsscript.html` used to count as the manifest, so the push went out with no manifest and the API rejected it with an unhelpful error. It is now refused before anything is sent, with an error that names the file and asks for `appsscript.json`.
- f20c719: `script +push` no longer deletes remote files that a local symlink stands for. It used to skip symlinks with a warning and then replace the whole project, so the remote copy of a symlinked `.gs`/`.js`/`.html` file (or of every file under a symlinked directory) was deleted. It now refuses the push and names those symlinks; replace each with a copy of its target. Symlinks whose names would not be pushed anyway (such as `README.md`) are still ignored.
- e0cd3a0: `script +push` now matches file extensions in any case. `Code.GS`, `Index.HTML` and `Lib.JS` used to be skipped without a word, and because a push replaces every file in the project, the matching remote file was deleted. They are now pushed as `Code`, `Index` and `Lib`. On a case-sensitive disk, `Code.gs` next to `Code.GS` is refused as two files mapping to one name.
- 8e3d166: `script +push` drops a leading UTF-8 byte order mark (written by Notepad and PowerShell) from each local file instead of sending it as part of the source, where it became an invisible character at the start of the code and the first byte of the `appsscript.json` manifest. `+pull` never writes one, so a pushed file now round-trips unchanged.
- 08ea2c8: `script +run` classifies a failed run by the code of its error status: an exception thrown by the script now exits 1 (HTTP 400, permanent) instead of 6 (retryable), and a run that hit its deadline (DEADLINE_EXCEEDED) still exits 6.
- 1cb10ff: Network error messages and debug logs no longer repeat the request's query string (such as an API key or a resumable upload ID) through the underlying HTTP error.
- ae36041: `gwsr auth setup --non-interactive` no longer reports success when the saved OAuth client belongs to a different GCP project than the one being set up. It now returns `action_required` with steps to replace the client, since the old client's project would otherwise stay the quota project.
- cb56057: Interactive `gwsr auth setup` no longer pre-fills the saved OAuth client ID when that client belongs to a different project than the one being set up. Before this fix, pressing Enter after switching projects saved the old project's client under the new project.
- 3a0d487: `gwsr auth setup` now validates the project ID from `--project`, from the wizard's "Enter a project ID" and "Create a new project" inputs, and from the current gcloud configuration. Before, any string was accepted. It was passed to `gcloud` as an argument, where a value such as `-q` or `--impersonate-service-account=...` is read as a flag. It was also put unescaped into the path of the consent-screen request sent with your gcloud access token, and saved as the quota project in the OAuth client file.
- cce6f22: `sheets +write --csv-file` and `sheets +append --csv-file` no longer drop blank lines. A blank line in the CSV is now an empty row that leaves that sheet row untouched, so the rows after it land where they are in the file instead of shifting up. Blank lines at the end of the file are still ignored.
- 1014a22: `sheets +read --output` writes a rectangular CSV: rows are padded with empty fields to the widest row. The Sheets API drops trailing empty cells, so a short row (or an empty one) followed by a wider row produced records with different field counts, which strict CSV readers such as pandas reject.
- 469de52: `gmail +watch` and `events +subscribe` no longer miss a Ctrl+C or SIGTERM that arrives between pull iterations. The signal was only delivered to loops already waiting at that instant, so one received while the loop was handling a response was lost and every later Ctrl+C/SIGTERM was swallowed too, leaving the stream running until killed. The shutdown request is now latched, so the loop stops at its next wait.
- 461d88b: `--upload` on a method whose simple upload is documented with `multipart: false` now sends `uploadType=media` (the raw file with its content type), and with `--json` metadata uses resumable when available or fails with a validation error before sending.
- 4acb358: The generated `gwsr-gmail-filter` skill documents the usage and flags of `+filter list`, `+filter create` and `+filter delete`; before, it showed only `gwsr gmail +filter` with no flags, which does not run.
- c8af160: Generated helper skills now carry the "write command, confirm with the user" caution for every helper that asks for confirmation (`--yes`), not only for a fixed list of names. `calendar +delete`, `+update` and `+rsvp`, `gmail +reply`, `+reply-all`, `+forward`, `+filter` and `+unsubscribe`, `sheets +clear`, `drive +share`, `script +run`, `admin +user-suspend` and `+group-add-member`, and `workflow +file-announce` were missing it.
- eab36f8: The `gwsr-docs-write` skill now shows that `docs +write` needs `--text` or `--text-file`. Its usage line was `gwsr docs +write --document-id <ID>`, which clap rejects, and the flags table marked both content flags as not required. A new test checks every committed helper skill against the helpers' own definitions, because the CI drift check only warns.
- bf27d9b: The generated `gwsr-workflow` and `gwsr-modelarmor` skills no longer tell agents to run `gwsr <service> <resource> <method>` or `gwsr schema <service>.<resource>.<method>`. Every command of these services is a `+helper`, so those commands failed; the skills now show `gwsr <service> +<helper> [flags]` and point at `gwsr <service> +<helper> --help`.
- 190c48d: `gwsr dev generate-skills --index` refuses a directory (an existing one, or a path ending in `/`) with a validation error (exit 3) before writing any skill. It used to write every skill and then fail with an internal error (exit 5) when writing the index.
- 7ce98cf: `gwsr dev generate-skills --index PATH` links each skill relative to the index file and `--output-dir`. The links were always `../skills/<name>/SKILL.md`, which only worked for the repository's own `docs/skills.md` + `skills/` layout and broke for any other index or output location.
- bad86c5: Generated service skills now list each API method on one Markdown line. Discovery descriptions with paragraphs or bullet lists (calendar `calendars.insert`, `calendars.transferOwnership`, `events.import`) no longer break out of the method list, and a method without a description (script `scripts.run`) no longer leaves a dangling `—` and trailing whitespace.
- a13bd2b: Skills installed one at a time now say what they depend on. Every service and helper skill lists `gwsr-shared` under `metadata.openclaw.requires.skills` and its prerequisite line gives the `npx skills add` command for `gwsr-shared`; recipe and persona skills say how to install any listed skill that is missing. The README's selective-install example installs `gwsr-shared` first.
- 295f7c1: Generated helper skills now show required one-of flag groups. The usage line includes a member of each group (for example `gwsr gmail +archive --message-id <ID>`), a line under it names the alternatives, and the flags table marks group members as "one" or "at least one" instead of "not required".
- 1f91651: Helper skills no longer split a tip that wraps over several help lines into one fragment bullet per line (for example "- a Model Armor failure sends nothing." in `gwsr-gmail-send`). Wrapped tips in helper `--help` now continue with a hanging indent, and the skills generator joins them into one bullet.
- d0f82cb: `slides +read` turns line breaks inside a paragraph (Shift+Enter, which Slides stores as a vertical tab) into newlines in `text` and `notes`, like `docs +read`. They were returned as U+000B, which terminal output drops as a control character, so the words on either side of the break ran together ("Q3 resultsAll regions").
- 6ea58d1: An unregistered API named without a version (`gwsr dlp`, `gwsr oauth2`) is reported as an unknown service again, with the hint to use `<api>:<version>`, instead of "Did you mean `gwsr help`?" or "`gwsr auth`?". The suggestion for a mistyped built-in command (`gwsr completion`, `gwsr comands`) now only appears for a near miss: one edit, or one per four characters.
- 42cc161: A `GWSR_TOKEN` that an API rejects with HTTP 401 is no longer "refreshed" by resending the same token: the command fails at once with an auth error (exit 2) that says the fixed token expired or was revoked. Other credential sources still refresh once after a 401.
- 802a5d9: `gwsr auth status` now reports the OAuth client that `gwsr auth login` would use. When `GWSR_CLIENT_ID`/`GWSR_CLIENT_SECRET` are set, `client.source` is `environment_variables` and `client.overrides` names the `client_secret.json` they shadow (previously the file was reported as if it were in use). A half-set pair is reported as `client_error`.
- ee601cf: `gwsr auth status` and `gwsr auth export` now name the active profile in their "sign in again" hint (`gwsr auth login --profile <name>`), as API commands already did, instead of always suggesting plain `gwsr auth login`, which signs in to a different profile when `--profile` selected this one.
- d2c9d83: `gwsr auth status` no longer reports `"authenticated": false, "verified": true` when Google's token endpoint answers 429 or 5xx; an outage is now reported as unverified (`verification_error`), the same as an unreachable endpoint.
- 97ceb0c: Stream diagnostics (`gmail +watch`, `events +subscribe`) and the default JSON log lines on stderr now escape C1 control characters (U+0080–U+009F), DEL and bidi, zero-width and line-separator characters as `\uXXXX`, as the error envelope already did. These lines carry API- and message-derived text, and some terminals act on those characters: U+009B is an 8-bit CSI. The decoded JSON is unchanged.
- cdd9587: `gmail +watch` and `events +subscribe` exit with the class of the last failure when they give up after `--max-failures` transient failures: a lost connection now exits 10 instead of 5, a token that can't be refreshed 2 instead of 5, and a 403 rate limit keeps its reason and exits 6 instead of 1. The error is no longer replaced by reason `transientFailureBudgetExhausted`.
- e4c1ff3: `gmail +watch` and `events +subscribe` now report the real number of failed attempts when they stop after `--max-failures`. With `--max-failures 2`, the error said "giving up after 2 consecutive transient failures" although three pulls had failed. It now says "giving up after 3 consecutive transient failures (--max-failures 2)".
- 5bdc900: `gmail +watch` and `events +subscribe` now apply `--jq`, `--format` and `--columns` to every streamed record instead of silently ignoring them.
- 406b05c: `gmail +watch` and `events +subscribe` now treat a 403 rate-limit error (`rateLimitExceeded`, `userRateLimitExceeded`, `RATE_LIMIT_EXCEEDED`) as a transient failure that backs off and counts against `--max-failures`, like a 429, instead of stopping the stream on an error that exits 6 (retryable).
- 7598bf2: `gmail +watch` and `events +subscribe` no longer retry a permanent `--sanitize` failure (for example a Model Armor 400, 403 or 404, or an invalid template) as transient until `--max-failures` runs out. The stream now stops on the first such error with its own exit code; network and retryable API failures are still retried.
- f579b77: `events +subscribe` and `gmail +watch` no longer die on a Ctrl+C or SIGTERM that arrives while they are still creating their Pub/Sub topic and subscription. Before, the signal killed the process before the stream had installed its handlers, so `--cleanup` never ran and the resources were left behind. Now setup finishes, the stream stops before its first pull, and `--cleanup` deletes what was created (without `--cleanup`, the reconnect hint is printed as usual).
- bd6e966: `events +subscribe` now acknowledges the events it already printed or wrote when a later event in the same pull fails (a `--jq` error, a closed stdout pipe such as `| head -n1`, a failed `--output-dir` write or a Model Armor failure). Before, nothing in the batch was acknowledged, so Pub/Sub redelivered the events that had been written and the stream printed them again.
- c5b1fe9: `events +subscribe` acknowledges the events it already printed when a Ctrl+C or SIGTERM arrives partway through a pulled batch (for example while `--sanitize` screens the next event). Before, the signal cancelled the whole batch before its acknowledgement, so Pub/Sub redelivered those events and a reattached stream printed them twice. A signal now cancels only a pending pull; the rest of the batch is not printed and stays unacknowledged.
- 65005d5: `events +subscribe` now fills each event's `type`, `source` and `time` from the `ce-type`, `ce-source` and `ce-time` Pub/Sub attributes that Workspace Events actually sends (the CloudEvents Pub/Sub binding). Before, it read unprefixed `type`/`source`/`time` attributes, so these fields were always `null` and `--jq 'select(.type == ...)'` matched nothing.
- 637d8f8: `events +subscribe --output-dir` no longer silently overwrites an existing event file. Files are named `<milliseconds>_<counter>.json`, and the counter restarts with every run, so two runs writing to the same directory could pick the same name (same millisecond, same counter) and the second one replaced the first run's event. Now a name that is already taken is skipped in favour of the next free counter value, and files are created without overwriting.
- 82f983d: `gmail +watch` and `events +subscribe` now reject setup flags that have no effect with `--subscription`, instead of silently ignoring them. With an existing subscription nothing is created, so `--cleanup` deleted nothing, and `--project`, `--topic` and `--label-ids` (`+watch`) or `--target`, `--event-types` and `--project` (`+subscribe`) were dropped. These combinations now fail with a validation error (exit `3`). The `events +subscribe` help example that combined `--subscription` with `--cleanup` now creates its resources with `--target`, `--event-types` and `--project`.
- 437f8f2: A mistyped built-in command (`gwsr completion bash`, `gwsr comands`) now says "Did you mean `gwsr completions`?" instead of reporting an unknown service and listing every service.
- 7d30a73: `drive +sync` now locks its directory (`.gwsr-sync.lock`) and refuses a second run into the same directory with exit 3, instead of letting the run that saves last drop the sync state the other recorded.
- 84e896d: `drive +sync` now lists a folder that it reaches through a second parent under `skipped`, with the path where it was mirrored, instead of silently leaving the second path empty.
- b200ee7: `drive +sync` stopped by Ctrl-C or SIGTERM now saves the sync state of the files it finished and exits 130 (SIGINT) or 143 (SIGTERM), so the next run no longer reports those files as conflicts.
- 3a3d1ff: `drive +sync` now fails when `--folder-id` does not exist, is no longer shared with you or is not a folder, instead of reporting an empty mirror and exiting 0.
- 30260a8: `drive +sync` no longer reports a file with two parents inside the synced folder as an untracked conflict on every run, and updates both of its local copies when it changes in Drive.
- 9d064f5: `drive +sync` again stops on a retryable API error (a rate limit such as `userRateLimitExceeded`, or a 500/503) and exits 6, instead of listing the file under `"failed"`, requesting every remaining file and exiting 1 with the non-retryable `syncPartialFailure`.
- 32b389a: Sync generated skills with the latest Google Discovery documents
- de3518f: `--format table --columns` on a single object (a `get` response or `--dry-run` output) lists the fields in the order given, as CSV and multi-row tables already do, instead of in the response's key order.
- 97e3517: `--format table` and `--format csv` cells, and raw `--jq` strings printed on a terminal, now keep the zero-width non-joiner and joiner (U+200C, U+200D). Before, they were removed, which changed the data: the emoji sequence 👨‍👩‍👧 became three separate emoji, and Persian and Indic text lost the joiners it needs. Other invisible, bidi and control characters are still removed, and a joiner in front of `=`, `+`, `-` or `@` does not get past the CSV formula guard.
- 32b4c30: `--format table` now keeps a key that contains a newline or tab on one line in the header and in key/value output, as it already did for values. Two keys that display the same after control characters are stripped now each show their own values.
- 4a38c61: `--format table` now measures cells in terminal columns instead of characters, so columns stay aligned after CJK text and emoji, the 60-column cap holds for wide characters, and truncation never cuts a flag, skin-tone emoji or accented letter in half.
- f2e4a20: `gwsr tasks +add --due` now rejects a time with fractional seconds past midnight (e.g. `2026-04-15T00:00:00.5Z`) instead of silently truncating it to the date.
- 2bd6d1a: Tests only: the client-file and project-ID error tests now check the exact field name and guidance. Before, they matched text that was already in the file path or in the echoed gcloud output.
- a9968cc: Tests only: every destructive helper (`sheets +clear`, `script +push`, `admin +user-suspend`, `gmail +filter delete`, `drive +share --role owner`, `calendar +delete`) is now checked end to end to refuse without `--yes`. Before, the `sheets +clear` test called the confirmation gate directly with a hard-coded impact and skipped itself on a terminal.
- b4cb852: Tests only: the Discovery cache test now checks that a successful fetch replaces an expired cache entry, not just that the document came from the network.
- 90b4f09: Tests only: four helper rejection tests now check the specific error and that no request was sent. Before, a request the mock server didn't match also failed (404), so they passed even without the validation they were named after.
- cdb019f: Tests only: the local-timezone fallback test now checks that the zone the OS reports is the one used. Before, it skipped every assertion whenever lookup failed, including when the lookup itself was broken.
- 5034494: Tests: the 401-refresh log test captures logs with a process-wide subscriber; a thread-scoped one raced with other test threads registering the same callsites and could miss the line.
- 37909b7: Tests only: the `auth logout` test now checks that an already-revoked token is reported as revoked, and that `--no-revoke` sends no revocation request.
- 18d8c3c: Tests: keep the "connection refused" port reserved for the whole test, so a server from a test running in parallel can't take it and answer.
- 46da177: Tests only: the HTTP retry tests now check that retries actually wait. Backoff must reach into the upper half of its jitter range, and a short `Retry-After` must be honoured.
- cf0d784: Tests only: the skill-tip sentence check now covers the helpers of every service. Before, its Discovery document had no API version, so only the workflow helpers were checked.
- 621fdcb: Tests only: the `drive +upload` path-confinement test now sets `GWSR_RESTRICT_PATHS=cwd` and uploads a file that exists. Before, it passed only because the file was missing.
- bffea18: Tests only: three validation property tests now generate inputs that reach the accept path and compare against an independent definition. Before, the API base URL and `GWSR_API_BASE_URL` properties never saw an accepted URL, and the dangerous-character property restated the function under test.
- a4b773e: Tests only: three helper tests (`gmail +filter create` flag rules, `events +renew` flag rules, and the check that every helper tip in the generated skills is a whole sentence) built their command tree without an API version. Since helpers are offered only for the registered version, they found no helper and passed without checking anything. They now use the registered version and fail if the helper is missing. No behavior change.
- ff99a1a: Text read with `docs +create/+write --text-file`, `sheets +append/+write --csv-file` and `admin +user-create --password-file` (from a file or `-` for stdin) no longer keeps a leading UTF-8 byte order mark, which Windows editors and PowerShell write. Before, `docs` inserted it into the document as an invisible character, and with `--markdown` it stopped a first-line `# Heading` from becoming a heading.
- 584433a: `--sanitize` now screens text responses of generated methods (for example `drive files export` to `text/plain`, or a text file fetched with `alt=media`) that are printed inline as JSON. Previously only JSON responses were screened, so text content reached stdout unscreened even in block mode. Bodies saved with `-o PATH` or streamed with `-o -` are unchanged.
- 1fed4df: The cached account time zone is now kept per identity (profile, credentials file, ADC file and `--impersonate` user) and is never cached for `GWSR_TOKEN`/`GWSR_TOKEN_FILE`. Previously one global cache file was shared, so after switching `--profile`, `gwsr auth use`, `GWSR_CREDENTIALS_FILE` or the `--impersonate` subject, calendar and workflow helpers used the previous account's time zone for up to 24 hours (wrong "today" window for `+agenda`/`+standup-report`, wrong event times for `+insert` without an offset). Existing cache files from older versions are ignored and removed on the next `gwsr auth login`/`logout`.
- bfa6a70: The cached account time zone is now kept per credentials file even when `GWSR_CREDENTIALS_FILE` or `GOOGLE_APPLICATION_CREDENTIALS` is a relative path or a symlink. The cache key used the path as written, so `./sa.json` in two project directories (two different accounts) shared one cached time zone for up to 24 hours.
- ea658da: When neither the account nor the local time zone can be determined, the account lookup's own error (for example a 403 missing-scope error, exit 1, with its scope hint) is now reported instead of being turned into a validation error (exit 3).
- 75e941b: When an API rejects a cached access token (HTTP 401) and the refreshed token comes back without an expiry, the rejected token is now removed from the token cache. Before, it stayed cached, and later commands were sent with it again until it expired.
- eeb478a: Access tokens are now cached per grant, not only per OAuth client. After `gwsr auth login`, a command that was already running with the previous login's refresh token could cache that grant's access token, and later commands would use it (possibly for another account or scope set). The cache key now includes a short SHA-256 fingerprint of the refresh token (never the token itself), so such an entry is never served to the new login. Tokens cached before this change are ignored and expire on their own.
  
  The cross-process lock also now notices when its lock file was deleted while waiting (as `gwsr auth logout` does): it relocks the current file, or fails with an error if the profile directory is gone, instead of holding a lock that excludes nobody.
- d27365c: `gwsr auth login` and `gwsr auth logout` now clear the access-token cache while holding its cross-process lock. Previously a concurrent `gwsr` process that was refreshing a token could write the previous grant's access token back after the cache was cleared, so a later command could use a token for the old account or scope set. `auth logout` now also lists the cache lock file in `removed`.
- 202e4f5: A token-endpoint answer that is not an OAuth error (reason `tokenEndpointError`) no longer comes with a hint that contradicts its message. A 401 like that, for example from a proxy, said "the credentials were not rejected" while the hint said "The access token was rejected. Sign in again". The hint now says that signing in again will not help, and points to the network path (a proxy or captive portal).
- 0a86c0d: A 429 or 5xx from Google's OAuth token endpoint (refreshing a login, a service-account token, or the login code exchange) now exits `6` (retryable, reason `tokenEndpointUnavailable`) instead of `2` ("sign in again"). The credentials were not rejected, so agents should retry with backoff rather than ask the user to log in. The error keeps only a short excerpt of the endpoint's response body.
- 582edd1: A token-endpoint answer that is not an OAuth response is no longer treated as Google rejecting the credentials. Examples are a proxy's `407 Proxy Authentication Required`, a captive portal's redirect or HTML page (even with HTTP 200), and an empty error body. `gwsr auth status` used to report these as `"authenticated": false, "verified": true`. Commands failed with exit code 2 ("sign in again") and showed a JSON parse or Content-Type error instead of the HTTP status. They are now reported like a token-endpoint outage. `auth status` says `"verified": false` and puts the error in `verification_error`. Commands exit 1, or 6 for retryable statuses as before, with a message that names the HTTP status and says the credentials were not rejected. Real OAuth errors such as `invalid_grant` and `invalid_client` are unchanged.
- 8ccdd68: Google's OAuth token endpoint failures are now classified with the same retryable-status set as API calls (408, 429, 500, 502, 503, 504). A token-endpoint 408 now exits `6` (`tokenEndpointUnavailable`, retryable) instead of `2` as a credential error, and a permanent 5xx such as 501 or 505 now exits `1` with reason `tokenEndpointError` and a message that no longer says "temporarily unavailable … retry later".
- fc1fdf7: A token-endpoint answer that is a JSON object but no usable token response (for example a `200 OK` without an `access_token`) is no longer reported as rejected credentials (exit 2, "sign in again"). Like an HTML page from a proxy, it is now a token-endpoint failure (`tokenEndpointError`, exit 1, or 6 for a retryable status) that keeps the HTTP status and says the credentials were not rejected. This applies to user refresh tokens, the login code exchange and service-account keys.
- ef3a550: `--profile NAME` together with `GWSR_TOKEN` or `GWSR_TOKEN_FILE` is now a configuration error (exit `8`), as it already was with `GWSR_CREDENTIALS_FILE`. Previously the explicit profile was silently ignored, and the command ran as whatever identity the access token belonged to, even if that profile did not exist. Unset one of them. Without `--profile`, the token still takes priority.
- 899589f: A token-endpoint error no longer repeats access, refresh or ID tokens from the response body. When Google's token endpoint (or a proxy in front of it) returned a token response that failed to parse, for example with `expires_in` as a string, the error on stderr quoted the body, live tokens included. In a JSON error body the values of token, secret and assertion fields are now shown as `[redacted]`. The rest of the body is still quoted.
- e48897a: `GWSR_TOKEN_FILE` is now checked like `GWSR_TOKEN`: after trimming surrounding whitespace, the file must hold exactly one token with no inner whitespace, control or non-ASCII characters. Before, a file with two tokens on separate lines, a JSON token response or a pasted `Bearer ` prefix was accepted and put into the `Authorization` header as is. That failed with an unrelated request-builder or HTTP 401 error, after a pointless token "refresh". It is now a configuration error (exit 8) that names the file and does not echo its contents.
- f8dbf41: The debug log line for a token refresh after HTTP 401 no longer records the request's query string, such as a resumable upload's `upload_id`.
- 93f36ad: `gmail +triage` help no longer calls it an "inbox" summary. Its default query, `is:unread`, searches all mail, archived messages included. The help now says so and shows `--query 'is:unread in:inbox'` for the Inbox only. Behavior is unchanged.
- 3fcdef7: Shortened method descriptions in `--help` and generated skills no longer end at a period inside a word. When the length limit fell right after the period of `developers.google.com` or `v1.5`, the text was cut there as if a sentence ended (`See the docs at developers.`); it now falls back to the last whole word with `…`. A period followed by a line break now counts as a sentence end, like one followed by a space.
- 5bcda6b: With `--page-limit` and `--jq`, the `{"_truncated":...}` marker that ends a truncated JSON run is no longer passed through the jq filter (where `--jq .name` printed `null`); it is printed as is.
- bab25e1: Auth commands no longer treat a stored file they cannot inspect (for example inside a profile directory left unsearchable by `sudo gwsr auth login`) as a missing one. Before, such a file was treated as absent. Now it is an error that names the path:
  
  - `auth logout` fails before removing anything instead of skipping the revocation;
  - `auth use` and `auth export` no longer say the profile has no credentials;
  - `auth list` no longer reports `"logged_in": false`;
  - a saved OAuth client file no longer falls back to another client;
  - `auth status` reports `invalid_profiles_error`, `client.overrides_error` or `note_error` instead of leaving the field out.
- 78a32f1: The error for an unknown `GWSR_*` environment variable no longer suggests running `gwsr --help` instead of unsetting it. The variable is checked before any command runs, so `gwsr --help` failed with the same error. The message now says to unset the variable first, and that `gwsr --help` then lists the supported variables.
- 4085777: A profile directory that cannot be read (for example one left owned by root by `sudo gwsr auth login`) is now a credential-store error (exit 9). Before, gwsr treated it as "not logged in": the default profile silently fell back to Application Default Credentials, a different identity, and an explicit profile reported that it had no credentials. An unreadable `GWSR_CREDENTIALS_FILE`, `GOOGLE_APPLICATION_CREDENTIALS` or gcloud ADC path is likewise reported as such instead of as missing.
- faff868: `gmail +unsubscribe` no longer prints the one-click URL's path and query in its network error (exit 10) when the sender's server cannot be reached. Unsubscribe URLs often carry a per-recipient token; the error now names the host only, as the main HTTP client's errors do.
- 096e361: `gmail +unsubscribe` now reads the `List-Unsubscribe` URIs from their angle brackets, as RFC 2369 defines them. A URI containing a comma (`?ids=1,2`) was cut at the comma, and an entry followed by a comment (`<https://…> (unsubscribe)`) kept the comment, so the one-click POST went to the wrong URL.
- 482a287: `gmail +unsubscribe` now refuses a one-click URL whose host is `localhost.` (with a trailing dot), any `*.localhost` name, or a single-label name written with a trailing dot (`intranet.`). The check for loopback and single-label hosts compared the host verbatim, so these spellings got past it. A DKIM-signed message could then make gwsr POST to a service on your own machine or network.
- 7c0eb99: `gmail +unsubscribe` now trusts only Gmail's own `Authentication-Results` header (the topmost one whose authserv-id is exactly `mx.google.com`) when checking for `dkim=pass`. A sender-supplied header lower in the message that claims `mx.google.com; dkim=pass` no longer enables the one-click POST, and `dkim=pass` must be an actual DKIM result rather than text inside a comment, another method's value or a look-alike authserv-id such as `mx.google.com.example.com`.
- 83dced8: `--upload` on a method whose Discovery document advertises only the resumable protocol now uses it for small files too, instead of failing because the method has no simple upload endpoint.
- 19e2dbf: Uploads (`--upload`) no longer crash when `--timeout` or `GWSR_TIMEOUT` is set to a huge value such as `18446744073709551615`. Adding the per-64-KiB upload allowance to the timeout overflowed. The timeout now saturates at the maximum.
- ee685d8: `--upload` on generated methods and `drive +upload` now infer the content type from the path you give, not from the file a symlink points to, as `gmail --attach` already did. Before, `notes.csv -> blob` was sent as `application/octet-stream`, and `drive +upload --convert` refused it.
- 5a9989d: `--upload` now refuses an `uploadType` in `--params` with a validation error (exit 3). Previously both were sent (for example `uploadType=media&uploadType=multipart`), so the server could store the multipart envelope as the file's content. `--dry-run` with `--upload` now lists the `uploadType` that is actually sent in `query_params`.
- d8723c8: `auth list` and `auth status` no longer silently hide profile directories whose names are not valid profile names (such as uppercase names from an older gwsr): they are reported under `invalid_profiles` with a warning, and `--profile Work` explains how to rename the directory so it can be used or logged out of.
- abf4a2d: Add regression tests for behaviors reported upstream: a POST with no body sends `Content-Length: 0` (`drive files download` no longer fails with 411), `calendar +agenda` reports all-day dates unshifted in time zones east of UTC, and `tasks +list --show-completed` also requests hidden tasks. No behavior change.
- 056a7cd: `--wait`: when polling a long-running operation fails (an `operations.get` error, a network failure or the wait timeout), the error now says that the request was accepted and names the operation it started, so a rerun no longer starts the operation a second time without warning. The error keeps its class and exit code. `--wait` on an API with no `operations.get` method is now refused before the request is sent, instead of after the operation has already started.
- 440df25: `--wait` on an API whose `operations.get` is nested (storage `b/{bucket}/operations/{operationId}`) now puts the operation id in the parameter after `operations/` and fills the other path parameters from the request, and refuses before sending when it cannot.
- cf48c67: `--wait` and `--wait-timeout` now appear only on methods whose operation the API's `operations.get` can poll. Before, any method returning a schema named `*Operation` offered them: on Compute Engine and other APIs without `operations.get`, `--wait` always failed, and `-o` tried to wait after the request was already sent. On Cloud DNS `managedZones.patch`, `--wait` was silently ignored.
- 5fc4fdc: A failed long-running operation (`--wait`, `events +subscribe`) now reports the HTTP status for its gRPC error code, and names the gRPC status in the message. Previously the raw gRPC code was reported as if it were an HTTP status (`"code": 14`), so a transient `UNAVAILABLE` or `RESOURCE_EXHAUSTED` failure exited with `1` ("don't retry") instead of `6`.
- 7c499ee: `--sanitize` now screens the result that `--wait` prints. Previously only the pending long-running operation was sent to Model Armor, and the finished operation's response was printed (or saved with `-o`) unscreened, even in `block` mode.
- 950025f: `--wait` now keeps polling until `--wait-timeout` is actually reached. It used to give up as soon as the next poll interval would end past the deadline, so `--wait-timeout 1` failed without polling even once, and longer timeouts stopped up to 10 seconds early.
- afc1f7e: `gmail +watch --subscription` accepts `--label-ids` again. The labels filter which messages are emitted, so they matter with an existing subscription too, but since the change that rejected setup flags with `--subscription`, the command refused them and could only stream every added message (sent mail, drafts, other labels). The `reconnect` hint printed after setup now includes the `--label-ids` the run used.
- 470a548: On Windows consoles that cannot render ANSI escape sequences (the classic console host without virtual terminal processing), errors and logs on stderr are now plain text instead of showing raw `←[1;31m` codes; gwsr turns virtual terminal processing on where the console supports it.
- df59b13: On Windows, `GWSR_*` and `RUST_LOG` environment variables are matched ignoring case like every other Windows program does, so `gwsr_restrict_paths=cwd` is applied (or rejected when invalid) instead of being silently ignored.
- 81323c0: On Windows, gcloud's Application Default Credentials are found in `%APPDATA%\gcloud\application_default_credentials.json`, where `gcloud auth application-default login` writes them, instead of a `~/.config/gcloud` path gcloud never uses there.
- c94f4ea: On Windows, a `gwsr` process waiting for the token-cache or key lock no longer ends up "holding" a lock on a file that `auth logout` deleted meanwhile, which let two processes into the locked section at once; like on Unix, it now reopens and locks the file that is actually at the path.
- 2398562: `GWSR_API_BASE_URL` now applies to the `workflow` helpers and to the account time zone lookup used by calendar and workflow helpers. Before, these always sent their requests (with the bearer token) to the public Google hosts, bypassing the configured private endpoint or recording proxy.
- 720872a: `workflow +standup-report`, `+weekly-digest`, `+email-to-task` and `+file-announce` `--dry-run` plans now list every query parameter the real run sends (the calendar time window, `singleEvents`, `orderBy`, `maxResults`, `metadataHeaders`, `fields`, `supportsAllDrives` and the Chat `requestId`), with `<placeholders>` for values only known at run time. Previously they left these out, so the plan did not describe the requests that would be sent.
- fc77eb0: `gwsr workflow +standup-report` and `+weekly-digest` no longer hang when the API repeats a `nextPageToken`, which now fails with an error, or returns an empty one, which ends pagination.
- 3e19250: `workflow +standup-report` and `+weekly-digest` count only real meetings: they leave out events you declined, cancelled events, and working-location, out-of-office, focus-time, birthday and other non-`default` calendar entries, which they used to list and count. `+meeting-prep` applies the same filter, so it no longer picks a timed focus-time or out-of-office block as your next meeting.
- 2423035: `workflow` helpers (`+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`) now honor `--sanitize` / `GWSR_SANITIZE_TEMPLATE`. Previously they ignored the Model Armor configuration and printed calendar, Gmail and Drive content unsanitized, even in `block` mode.
- 8e0357b: The `--yes` help no longer says it is "required when not on a terminal". It now says what the confirmation gate does: when stdin or stderr is not a terminal, destructive actions require `--yes`, and outbound ones (send, share, notify, run code, such as `gmail +reply` or `admin +group-add-member`) require it only when `GWSR_REQUIRE_CONFIRM=1`. The generated skills, including the method-flag table in `gwsr-shared`, use the same text.
- 4a683fc: Update the yanked `yoke-derive` 0.8.3 dependency to 0.8.4 so dependency audits pass.

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
