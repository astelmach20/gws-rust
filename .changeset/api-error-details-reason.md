---
"gws-rust": patch
---

API errors in Google's current error format now take their `reason` from the `google.rpc.ErrorInfo` entry in `details` instead of the coarse `status`. A 403 `RATE_LIMIT_EXCEEDED` now exits with the retryable code `6` (it was `1`, even though the request had already been retried as rate-limited), and a `SERVICE_DISABLED` error now includes `enable_url` and the "API not enabled" hint.
