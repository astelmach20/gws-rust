---
"gws-rust": patch
---
`--columns` with `--page-items` or `gwsr batch` no longer fails when the first item lacks a selected field: that cell is empty, and only a column no item has is an error, reported after the rows.
