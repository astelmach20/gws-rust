---
"gws-rust": patch
---

`gwsr auth status` now reports the OAuth client that `gwsr auth login` would use. When `GWSR_CLIENT_ID`/`GWSR_CLIENT_SECRET` are set, `client.source` is `environment_variables` and `client.overrides` names the `client_secret.json` they shadow (previously the file was reported as if it were in use). A half-set pair is reported as `client_error`.
