---
"gws-rust": patch
---

`--page-all` (and `--page-items`) no longer loops forever when an API returns a `nextPageToken` it has already returned. The pages fetched so far are still printed, then the command fails with an error that names the repeated token, instead of sending requests without end under the default unlimited `--page-limit`.
