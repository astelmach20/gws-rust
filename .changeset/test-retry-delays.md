---
"gws-rust": patch
---

Tests only: the HTTP retry tests now check that retries actually wait. Backoff must reach into the upper half of its jitter range, and a short `Retry-After` must be honoured.
