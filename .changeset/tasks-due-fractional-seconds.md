---
"gws-rust": patch
---

`gwsr tasks +add --due` now rejects a time with fractional seconds past midnight (e.g. `2026-04-15T00:00:00.5Z`) instead of silently truncating it to the date.
