---
"gws-rust": patch
---

Tests: keep the "connection refused" port reserved for the whole test, so a server from a test running in parallel can't take it and answer.
