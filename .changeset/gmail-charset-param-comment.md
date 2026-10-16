---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` ignore a comment in a body part's `Content-Type`. `charset=iso-8859-1 (latin)` was read as the unknown charset `iso-8859-1 (latin)`, so the body was decoded as UTF-8 and its accented characters came out as `�`.
