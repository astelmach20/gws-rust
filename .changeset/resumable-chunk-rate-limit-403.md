---
"gws-rust": patch
---
A resumable upload chunk answered with a Google rate-limit 403 (`rateLimitExceeded`/`userRateLimitExceeded`) is now retried like a 429, instead of failing the upload on the first answer.
