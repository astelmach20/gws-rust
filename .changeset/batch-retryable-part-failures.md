---
"gws-rust": patch
---

`gwsr batch` now exits `6` (retry later) when every failed call failed with a retryable error (HTTP 408, 429, 500, 502, 503, 504, or a 403 rate limit such as `userRateLimitExceeded`), the same code each call would get on its own. Before, any per-call failure exited `1` (permanent), so a script could not tell that running the batch again would help. If any call failed permanently, the exit code is still `1`. The reason stays `batchPartialFailure`.
