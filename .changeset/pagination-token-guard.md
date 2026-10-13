---
"gws-rust": patch
---

`workflow +meeting-prep`, `events +renew --all` and `gmail +watch` history paging now share one page-token rule: an empty `nextPageToken` ends pagination, and any token the API repeats is an error instead of an endless loop.
