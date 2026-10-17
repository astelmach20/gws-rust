---
"gws-rust": patch
---

`--format csv --columns` now applies to rows that are not objects (Sheets `values`, a list of IDs) instead of silently ignoring the selection and printing every row whole. As with `--format table`, such rows have the single column `value`, and naming any other column is an error (exit 3).
