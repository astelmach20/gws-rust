---
"gws-rust": patch
---

`docs +read --markdown` no longer ends a paragraph with a literal `\` when its last line break (Shift+Enter) is followed only by spaces or tabs. The trailing break is dropped, as it already was when nothing followed it.
