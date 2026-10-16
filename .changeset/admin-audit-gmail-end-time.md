---
"gws-rust": patch
---

`admin-reports +audit --application gmail` now works without `--until`. The Reports API rejects Gmail activity queries that have no `endTime`, so the helper sends the current time when `--until` is omitted, and refuses a Gmail range longer than the API's 30-day limit with a validation error (exit 3) instead of a 400 from the API.
