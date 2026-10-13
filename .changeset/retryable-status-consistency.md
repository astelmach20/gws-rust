---
"gws-rust": patch
---

Exit code 6 (retryable) now covers exactly the statuses gwsr itself retries: a 408 Request Timeout exits 6 (was 1), and a 501 Not Implemented or other non-transient 5xx exits 1 (was 6).
