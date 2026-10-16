---
"gws-rust": patch
---

`--format csv` writes a rectangular CSV for arrays of rows, such as Sheets `values` from `sheets +read` or `spreadsheets values get`. The Sheets API drops trailing empty cells and returns an empty row as `[]`, so rows used to have different field counts, and strict CSV readers such as pandas rejected the file. Each row is now padded with empty fields to the width of the widest row.
