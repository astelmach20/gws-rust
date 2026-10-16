---
"gws-rust": patch
---

`admin-reports +audit --application gmail --since 30d` no longer fails with "Gmail audit queries cover at most 30 days". The default end time (now) is taken in whole seconds, like the look-back it counts from, so a 30-day look-back spans exactly 30 days instead of 30 days and a fraction of a second.
