---
"gws-rust": patch
---

`--page-all` adds the top-level `nextPageToken` to a `fields` mask that only names it inside a sub-selection (for example `files(id,nextPageToken)`), instead of stopping silently after the first page.
