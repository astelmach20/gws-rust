---
"gws-rust": patch
---

`calendar +freebusy` now exits `6` (retryable) when Google reports a per-calendar `backendError` or `internalError`, instead of exit `3` (validation error).
