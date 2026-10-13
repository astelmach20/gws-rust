---
"gws-rust": patch
---

`gwsr drive +share` now rejects `--message` with `--domain`/`--anyone`, `--role owner` for a group or domain, and `--role owner --no-notify` (Drive requires the notification) before asking for confirmation or loading credentials, and `drive +download`/`+export` check an explicit `--output` first, so these all exit with a validation error (3) instead of a confirmation (7) or authentication (2) error.
