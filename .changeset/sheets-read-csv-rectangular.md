---
"gws-rust": patch
---

`sheets +read --output` writes a rectangular CSV: rows are padded with empty fields to the widest row. The Sheets API drops trailing empty cells, so a short row (or an empty one) followed by a wider row produced records with different field counts, which strict CSV readers such as pandas reject.
