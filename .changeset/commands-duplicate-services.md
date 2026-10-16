---
"gws-rust": patch
---

`gwsr commands` lists a service once when it is named twice or by two of its aliases (`gwsr commands reports admin-reports`). It used to fetch it twice and print a duplicate entry in `services` (a duplicate set of rows with `--format table|csv`).
