---
"gws-rust": patch
---

`--profile NAME` together with `GWSR_TOKEN` or `GWSR_TOKEN_FILE` is now a configuration error (exit `8`), as it already was with `GWSR_CREDENTIALS_FILE`. Previously the explicit profile was silently ignored, and the command ran as whatever identity the access token belonged to, even if that profile did not exist. Unset one of them. Without `--profile`, the token still takes priority.
