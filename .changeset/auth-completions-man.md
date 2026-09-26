---
"gws-rust": patch
---

Shell completion now completes `gwsr auth` subcommands and their flags (`gwsr auth lo<TAB>` offers `login`/`logout`, `gwsr auth status --<TAB>` offers `--offline`), and `gwsr dev man` writes a page for each `auth` subcommand (`gwsr-auth-login.1`, `gwsr-auth-status.1`, ...). Previously both saw only `auth`'s passthrough argument, so nothing under `auth` was completed or documented.
