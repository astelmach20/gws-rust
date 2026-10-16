---
"gws-rust": patch
---

`events +renew --all` now exits `6` (retry later) when every subscription it failed to renew failed with a retryable error (429, 503, a 403 rate limit, ...), the same class a single `--subscription-id` renewal gets. It used to exit `1` even then, so a cron job had no signal that running again would help. When any failure is permanent (a 404, say), the run still exits `1`. Either way the error's reason is `renewPartialFailure` and the report lists every failure.
