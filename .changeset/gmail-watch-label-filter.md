---
"gws-rust": patch
---

`gmail +watch --label-ids` now emits only messages that carry one of the watched labels. Previously the label filter only decided which mailbox changes triggered a notification; every message added since the last notification was then emitted, including sent mail, drafts and messages under other labels.
