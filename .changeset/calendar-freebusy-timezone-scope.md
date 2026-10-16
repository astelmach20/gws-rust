---
"gws-rust": patch
---

`calendar +freebusy` once again uses the account time zone when the profile was granted `calendar.freebusy`. Its token was narrowed to that scope alone, and Calendar refuses that scope for `settings.get`, so the time zone lookup failed and the command fell back to the machine's local zone. That changed how `--start`/`--end` without an offset and the reported free slots were read. The helper now also asks for `calendar.readonly`, which `settings.get` accepts.
