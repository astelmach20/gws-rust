---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` decode a `charset=utf-16` message body that has no byte-order mark as big-endian, as RFC 2781 requires. It was read as little-endian and came out as unrelated CJK characters. A byte-order mark still decides the byte order, and `utf-16le` / `utf-16be` keep their own.
