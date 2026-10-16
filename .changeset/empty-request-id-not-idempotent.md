---
"gws-rust": patch
---

A POST/PATCH whose `requestId` (in `--params`, the body or a `gwsr batch` line) is empty, blank or null is no longer treated as safe to retry. Such an id gives the API nothing to de-duplicate on, so retrying after a 503 or a dropped connection could apply the call twice, for example by posting a Chat message twice. Only a non-blank `requestId` makes the call retryable.
