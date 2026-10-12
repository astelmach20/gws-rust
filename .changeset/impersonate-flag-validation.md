---
"gws-rust": patch
---

`--impersonate` now rejects values that are not email addresses (exit 3), as `GWSR_IMPERSONATE` already did, instead of passing them into the service-account assertion, `auth status` output and cache keys.
