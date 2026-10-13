---
"gws-rust": patch
---

`docs +read --body-format markdown` no longer puts a literal `\` inside code: a line break (Shift+Enter) in monospace text now ends the code span and the next line starts a new one, instead of reading as `a\ b`.
