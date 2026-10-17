---
"gws-rust": patch
---

`gmail +reply-all --remove` now checks its addresses the way `--to`, `--cc` and `--bcc` do. Control characters (CR/LF included) and invisible or bidi Unicode in the value are an error (exit 3); before, they were silently stripped, or the address matched no recipient.
