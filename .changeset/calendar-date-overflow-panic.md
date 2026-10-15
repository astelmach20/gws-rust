---
"gws-rust": patch
---
`calendar +insert`, `+freebusy` and `+update` now reject an all-day date at the end of the representable range (e.g. `--start +262142-12-31`) with a validation error instead of panicking.
