---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` now treat a 403 rate-limit error (`rateLimitExceeded`, `userRateLimitExceeded`, `RATE_LIMIT_EXCEEDED`) as a transient failure that backs off and counts against `--max-failures`, like a 429, instead of stopping the stream on an error that exits 6 (retryable).
