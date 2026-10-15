---
"gws-rust": patch
---

Security: `--format csv` now neutralises a spreadsheet formula in a field whose value is an array. An array is written as one cell with its elements joined by `, `, so a value such as `["=HYPERLINK(...)"]` in API data became the cell `=HYPERLINK(...)` without the `'` prefix, and a spreadsheet would run it as a formula. Array cells starting with `-` (for example `[-5, 3]`) now also get the `'` prefix; single numbers are unchanged.
