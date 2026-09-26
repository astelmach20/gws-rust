---
"gws-rust": patch
---

`calendar +agenda --calendar-id` now fails with a validation error naming every requested calendar ID that is not in your calendar list. Previously, when at least one requested ID matched, the others were silently dropped and the agenda looked complete.
