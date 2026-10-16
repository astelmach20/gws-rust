---
"gws-rust": patch
---

An API error's `code` and exit code now follow the HTTP status, not the `error.code` in the response body. The retry policy already follows the HTTP status. When the two disagree, for example HTTP 503 with a body code of 400, a request was retried as transient but then exited `1` (permanent). An HTTP 400 with a body code of 503 was never retried but exited `6` (retryable). Both now agree with the HTTP status.
