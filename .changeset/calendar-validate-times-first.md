---
"gws-rust": patch
---

`gwsr calendar +insert`, `+agenda`, `+freebusy` and `+update` now check `--timezone`, `--start`/`--end`/`--duration`, `--slot` and `--working-hours` (and, for `+update`, that something changes) before asking for confirmation or loading credentials. These now exit with a validation error (3), where they used to exit with a confirmation (7) or authentication (2) error after a time-zone lookup.
