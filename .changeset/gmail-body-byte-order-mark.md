---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` remove a byte-order mark from a message body instead of keeping it as an invisible U+FEFF at the start of the text (and of the quoted original), and decode a `charset=utf-16` body in the byte order its BOM gives. A big-endian UTF-16 body was read as little-endian and came out as unrelated CJK characters.
