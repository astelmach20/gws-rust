---
"gws-rust": patch
---

`calendar +update` can switch an event between all-day and timed. The Calendar API merges a patched `start`/`end` into the event's current one, so a new all-day date left the old `dateTime` in place (and a new time left the old `date`). The patch now clears the other field.
