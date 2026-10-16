---
"gws-rust": patch
---

`calendar +update --dry-run` now lists the GET of the current event that a real run sends before its PATCH when it merges `--add-attendee`/`--remove-attendee` or keeps the event's time zones. The plan also notes that times with an offset keep each end's current time zone.
