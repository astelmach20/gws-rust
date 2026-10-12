---
"gws-rust": patch
---

Tests: the shared OAuth HTTP client no longer reuses idle connections in unit tests, fixing intermittent "error sending request" failures in token tests.
