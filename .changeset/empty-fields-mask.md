---
"gws-rust": patch
---

An empty `--fields` mask is now rejected (exit `3`) before anything is sent. Previously it was sent as an empty `fields` parameter, and with `--page-all` or `--page-items` it became the malformed mask `nextPageToken,`.
