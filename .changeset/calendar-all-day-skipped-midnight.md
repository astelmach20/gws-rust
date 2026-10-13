---
"gws-rust": patch
---

`calendar +freebusy` with date-only `--start`/`--end` no longer fails in time zones whose clocks spring forward across midnight (for example America/Santiago on 2026-09-06): the day starts at its first existing local time. `calendar +agenda` also sorts an all-day event on such a day before that day's timed events instead of last.
