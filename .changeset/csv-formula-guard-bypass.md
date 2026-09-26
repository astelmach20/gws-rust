---
"gws-rust": patch
---

Security: `--format csv` now neutralises spreadsheet formulas that start after a stripped control or invisible character. Before, a value such as `<BEL>=HYPERLINK(...)` or `<zero-width space>@SUM(...)` in API data had the leading character removed but did not get the `'` prefix, so a spreadsheet would run it as a formula.
