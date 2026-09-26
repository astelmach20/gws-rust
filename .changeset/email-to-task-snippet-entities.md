---
"gws-rust": patch
---

`gwsr workflow +email-to-task` now decodes HTML entities in the Gmail snippet before using it as the task notes, so notes read `Tom & Jerry's` instead of `Tom &amp; Jerry&#39;s`.
