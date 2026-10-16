---
"gws-rust": patch
---

`--sanitize` no longer treats a Model Armor result whose filters did not all run as clean. A `NO_MATCH_FOUND` verdict now counts only with `invocationResult: SUCCESS`; `PARTIAL` or `FAILURE` invocations, and an unspecified or unknown `filterMatchState`, are sanitization failures, so `block` mode withholds the output (and sends nothing for the Gmail send helpers) and `warn` mode annotates it with `_sanitization.error`. A `MATCH_FOUND` is still a match even when some filters were skipped.
