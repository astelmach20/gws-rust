---
"gws-rust": patch
---

Internal documentation fixes: the confirmation gate docs now list every accepted `GWSR_REQUIRE_CONFIRM` value (`1`/`true`/`yes`/`on`), and the `gwsr batch` docs list the exact retryable statuses (408, 429, 500, 502, 503, 504, or a 403 rate limit) instead of "5xx". No behavior change.
