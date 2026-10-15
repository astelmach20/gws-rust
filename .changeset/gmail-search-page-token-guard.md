---
"gws-rust": patch
---
`gmail +search` and `gmail +triage` now stop on an empty `nextPageToken` instead of listing the first page again and returning duplicate messages, and fail on a repeated token instead of looping forever.
