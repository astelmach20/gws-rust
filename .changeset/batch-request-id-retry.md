---
"gws-rust": patch
---

`gwsr batch` now retries a batch request after a retryable failure (503, timeout, connection reset) when its POST/PATCH calls carry a `requestId`, as a single call already does. A batch with a POST or PATCH call that has no `requestId` is still never re-sent, because the server may already have applied it.
