---
"gws-rust": patch
---

When `gwsr` reads the body of an HTTP 403 to check for a rate-limit reason, `--timeout` now limits each gap between chunks, as it does for every other response body (and as the README says). Before, it limited the whole read, so a 403 body that arrived slowly but steadily failed with a false "no data received" error.
