---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` no longer turn a whole `ISO-2022-KR`, `HZ-GB-2312` or `ISO-2022-CN` message body into a single `�`. These charsets cannot be decoded, so the body is now decoded as UTF-8 with a warning, as for an unknown charset, which keeps its ASCII text (words, numbers, links).
