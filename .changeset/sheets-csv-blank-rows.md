---
"gws-rust": patch
---

`sheets +write --csv-file` and `sheets +append --csv-file` no longer drop blank lines. A blank line in the CSV is now an empty row that leaves that sheet row untouched, so the rows after it land where they are in the file instead of shifting up. Blank lines at the end of the file are still ignored.
