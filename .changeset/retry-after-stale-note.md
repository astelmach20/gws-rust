---
"gws-rust": patch
---

API errors no longer carry a misleading retry note. When an earlier attempt was retried (for example a 503 with `Retry-After`) and the final answer was a non-retryable error such as a 404, the error message said "gave up after N attempts; server asked to retry after Ns". The note now appears only when `gwsr` actually stopped retrying a retryable failure, and the `Retry-After` it quotes is the final response's own.
