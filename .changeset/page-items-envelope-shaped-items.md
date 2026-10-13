---
"gws-rust": patch
---

`--page-items --format csv|table` (and `gwsr batch` with those formats) now print every item as one row. An item shaped like a list envelope, such as a Drive file selected with `--fields 'files(kind,parents)'`, was split into rows of its own array field and lost its other fields.
