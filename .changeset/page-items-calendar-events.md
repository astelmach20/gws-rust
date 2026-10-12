---
"gws-rust": patch
---

`--page-items` now detects the list field of Calendar `events list` (and other responses with list-metadata arrays such as `defaultReminders`) instead of failing with "several array fields".
