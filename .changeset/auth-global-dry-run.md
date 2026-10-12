---
"gws-rust": patch
---

`--dry-run` given before `auth` is no longer silently ignored. `gwsr --dry-run auth logout` used to delete the profile's credentials and revoke the refresh token for real (and `login`, `use` and `export` ran for real too). The auth subcommands without a dry-run mode now refuse it with a validation error (exit 3) and do nothing; `gwsr --dry-run auth setup` passes the flag on to setup's own dry run.
