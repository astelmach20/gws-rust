---
"gws-rust": patch
---

`docs +read` Markdown keeps a heading or title that contains a line break (Shift+Enter) in one heading, writing the break as a space. Before, the break was written as a hard break, which an ATX heading cannot hold: the heading ended with a literal `\` and the text after the break became a separate paragraph.
