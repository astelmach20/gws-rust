---
"gws-rust": patch
---

Add regression tests for behaviors reported upstream: a POST with no body sends `Content-Length: 0` (`drive files download` no longer fails with 411), `calendar +agenda` reports all-day dates unshifted in time zones east of UTC, and `tasks +list --show-completed` also requests hidden tasks. No behavior change.
