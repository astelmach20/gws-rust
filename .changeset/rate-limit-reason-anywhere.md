---
"gws-rust": patch
---

A 403 whose error body names a rate-limit reason after another reason (a later `errors[]` or `details[]` entry) now exits 6 (retryable) with that rate-limit reason, matching the client, which already retried it; it exited 1.
