---
"gws-rust": patch
---
`events +subscribe` and `gmail +watch` exit with the real error class when creating, authorizing or deleting Pub/Sub resources fails: a network failure is exit 10 instead of 5, a refused token is exit 2 instead of 5, and a 403 rate limit stays retryable (exit 6) instead of becoming exit 1.
