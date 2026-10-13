---
"gws-rust": patch
---

`docs +read` Markdown keeps monospace text that contains backticks as one code span. Before, ``a`b`` in a code font became `` `a`b` ``, which Markdown reads as the code `a` followed by a literal ``b` ``.
