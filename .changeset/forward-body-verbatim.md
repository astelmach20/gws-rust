---
"gws-rust": patch
---

`gmail +forward --body` now keeps the note verbatim, as `+send` and `+reply` keep `--body`. Before, leading and trailing whitespace and blank lines were trimmed, which lost the indentation of a first line or a list. A blank note is still treated as no note.
