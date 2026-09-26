---
"gws-rust": patch
---

`forms +responses --output` now guards its CSV against formula injection, like `--format csv` does: a response or question title starting with `=`, `+`, `-` or `@` gets a leading `'`, so a respondent's answer such as `=HYPERLINK(...)` no longer becomes a live formula when the exported file is opened in a spreadsheet. JSON output is unchanged.
