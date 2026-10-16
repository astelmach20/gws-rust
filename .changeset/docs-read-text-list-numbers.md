---
"gws-rust": patch
---

`docs +read --body-format text` now numbers ordered list items the way the document shows them (`1.`, `2.`, `3.`). Numbering is per list and nesting level, and a sub-list restarts under each new parent item. Every item used to read `1.`, and plain text has no renderer to correct that. Markdown output keeps `1.`, which CommonMark numbers itself.
