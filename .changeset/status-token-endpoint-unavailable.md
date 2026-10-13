---
"gws-rust": patch
---

`gwsr auth status` no longer reports `"authenticated": false, "verified": true` when Google's token endpoint answers 429 or 5xx; an outage is now reported as unverified (`verification_error`), the same as an unreachable endpoint.
