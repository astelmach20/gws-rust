---
"gws-rust": patch
---

A `GWSR_TOKEN` that an API rejects with HTTP 401 is no longer "refreshed" by resending the same token: the command fails at once with an auth error (exit 2) that says the fixed token expired or was revoked. Other credential sources still refresh once after a 401.
