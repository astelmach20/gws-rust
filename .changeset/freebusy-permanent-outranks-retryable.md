---
"gws-rust": patch
---

`calendar +freebusy` no longer exits `6` (retry later) when one calendar hit a transient Google error but another failed permanently (for example `notFound`, a calendar you can't see). Retrying can't fix the permanent failure, so the run now exits `3` and names every failed calendar. It still exits `6` when every failure is transient (`backendError`, `internalError`).
