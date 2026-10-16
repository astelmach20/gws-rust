---
"gws-rust": patch
---

An invalid `profile` in `config.toml` is now a configuration error (exit 8) for every command, as an invalid value of any other key already is. It used to load without error while `--profile` or `GWSR_PROFILE` overrode it, and for commands that load no credentials (`schema`, `cache clear`, `--dry-run`). To fix a file that now fails, set `profile` to a valid name: 1-64 lowercase letters, digits, `-`, `_` or `.`.
