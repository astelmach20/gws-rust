---
"gws-rust": patch
---

`calendar +update` keeps the event's own time zone when it moves the event with times that carry an offset (`--start`/`--end` like `2026-06-17T10:00:00-06:00`, or `--duration` alone) and no `--timezone`. It used to send the new `start`/`end` without a `timeZone`, dropping the event's zone (recurring events need one). Each end now keeps its current `timeZone`; `--timezone` still overrides it.
