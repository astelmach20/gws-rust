---
"gws-rust": patch
---

`calendar +insert` and `calendar +update` now send the time zone with a local (offset-less) end time when the start time has an offset. Previously the end was sent as `"timeZone": null`, so the API rejected the request; this happened for `+insert --start <RFC 3339> --end <local time>` and for `+update --end <local time>` without `--start`, which reuses the event's current start.
