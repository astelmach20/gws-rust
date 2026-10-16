---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` decode bodies labelled with common charset aliases that are not WHATWG labels, such as Windows code page names (`cp932`, `cp936`, `cp949`, `cp950`, `cp874`, `cp65001`, `cp2859x`) and `latin-1` / `latin9`. These were decoded as UTF-8 and came out garbled. Charsets with no decoder (`utf-7`, `cp437`, `cp850`) are still decoded as UTF-8 with a warning.
