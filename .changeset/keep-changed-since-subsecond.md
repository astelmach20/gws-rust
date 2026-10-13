---
"gws-rust": patch
---

`keep +list --changed-since` keeps the fractional seconds of the given time. They were dropped, which moved the bound earlier and returned notes changed before the requested time; passing a note's own `updateTime` returned that same note again.
