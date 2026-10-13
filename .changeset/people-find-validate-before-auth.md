---
"gws-rust": patch
---

`gwsr people +find` now rejects an empty `--query` or a contact-search `--limit` above 30 before loading credentials, so it exits with a validation error (3) instead of an authentication error (2).
