---
"gws-rust": patch
---
`events +renew` no longer reports a failed renewal as an internal error (exit 5): a single `--subscription-id` exits with the API error's own code (for example 6 for a retryable 503, or 1 for a 404), and a partial `--all` failure exits 1 with reason `renewPartialFailure`.
