---
"gws-rust": patch
---

`--format table` and `--format csv` cells, and raw `--jq` strings printed on a terminal, now keep the zero-width non-joiner and joiner (U+200C, U+200D). Before, they were removed, which changed the data: the emoji sequence 👨‍👩‍👧 became three separate emoji, and Persian and Indic text lost the joiners it needs. Other invisible, bidi and control characters are still removed, and a joiner in front of `=`, `+`, `-` or `@` does not get past the CSV formula guard.
