---
"gws-rust": patch
---

`gwsr auth login` now rejects `-s/--services` combined with `--scopes` (exit 3) instead of silently ignoring `-s`. `-s` filters the presets (`--write`, `--full`, the read-only default and the picker); `--scopes` is an exact list. To add scopes for more services, run another `gwsr auth login` (logins are incremental).
