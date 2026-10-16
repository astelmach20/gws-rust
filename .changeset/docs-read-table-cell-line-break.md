---
"gws-rust": patch
---

`docs +read` Markdown no longer leaves a literal `\` in a table cell whose text has a line break (Shift+Enter). The break was written as a hard break, but a table cell is one line, so its lines are joined with a space and the `\` stayed in the cell (`| one\ two |`). It is now `| one two |`.
