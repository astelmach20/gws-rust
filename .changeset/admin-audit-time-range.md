---
"gws-rust": patch
---

`admin-reports +audit` checks its time range locally: a `--since` that is not before `--until`, or that is in the future, is a validation error (exit 3) before any request, instead of a 400 from the Reports API. A `--since` look-back (`24h`, `7d`, and the `1d` default) now counts back from `--until` when it is given, so `--until` alone with an older date queries the day before it rather than failing because the start was still a day before now.
