---
"gws-rust": patch
---

Tests: the 401-refresh log test captures logs with a process-wide subscriber; a thread-scoped one raced with other test threads registering the same callsites and could miss the line.
