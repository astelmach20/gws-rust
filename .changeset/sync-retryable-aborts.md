---
"gws-rust": patch
---

`drive +sync` again stops on a retryable API error (a rate limit such as `userRateLimitExceeded`, or a 500/503) and exits 6, instead of listing the file under `"failed"`, requesting every remaining file and exiting 1 with the non-retryable `syncPartialFailure`.
