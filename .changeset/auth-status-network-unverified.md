---
"gws-rust": patch
---

`gwsr auth status` no longer reports `"authenticated": false, "verified": true` when Google's token endpoint cannot be reached. Without a response nothing was verified, so the report now matches `--offline`: `"authenticated": true, "verified": false`, with the network failure in `verification_error`. A credential that Google actually rejects is still reported with `token_error`.
