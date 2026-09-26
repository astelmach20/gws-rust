---
"gws-rust": patch
---

`admin-reports +audit --since` no longer crashes (exit 101) on a value whose last character is not ASCII (such as `7é`) or on a look-back too large for the calendar (such as `99999999999999d`). These values are now rejected as validation errors (exit 3).
