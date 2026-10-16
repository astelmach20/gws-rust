---
"gws-rust": patch
---
`gmail +watch` and `events +subscribe` exit with the class of the last failure when they give up after `--max-failures` transient failures: a lost connection now exits 10 instead of 5, a token that can't be refreshed 2 instead of 5, and a 403 rate limit keeps its reason and exits 6 instead of 1. The error is no longer replaced by reason `transientFailureBudgetExhausted`.
