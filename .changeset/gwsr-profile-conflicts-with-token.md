---
"gws-rust": patch
---

`GWSR_PROFILE` together with `GWSR_TOKEN`, `GWSR_TOKEN_FILE` or `GWSR_CREDENTIALS_FILE` is now a configuration error (exit 8), as `--profile` already was. The token or credentials file used to win silently, so the command ran as another identity than the profile you selected. A profile pinned in `config.toml` with `gwsr auth use` is still overridden by those variables.
