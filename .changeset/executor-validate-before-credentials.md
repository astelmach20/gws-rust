---
"gws-rust": patch
---

Generated method commands and `gwsr batch` now load credentials only after validating `--params`/`--json`/`--upload` and passing the confirmation gate. Bad input is a validation error (3) and a refused destructive call is a confirmation error (7), even when credentials are broken or a token would need a refresh; before, these were credential errors (2 or 8) and could trigger a token refresh first.
