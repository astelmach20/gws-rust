---
"gws-rust": patch
---

`gwsr batch` now splits each response part's headers from its body at the first blank line, whether it ends in `\r\n` or `\n`. Before, a part with `\n` headers whose JSON body contained `\r\n\r\n` was split inside the body and failed with "malformed batch part status line".
