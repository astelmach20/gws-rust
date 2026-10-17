---
"gws-rust": patch
---

`calendar +freebusy` now fails when a `--calendar-id` group cannot be expanded (for example `groupTooBig`), instead of reporting free slots that silently leave out every member of that group. Transient expansion errors (`backendError`, `internalError`) exit 6 like per-calendar ones.
