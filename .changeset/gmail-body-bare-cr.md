---
"gws-rust": patch
---

Gmail `+send`, `+reply`, `+reply-all` and `+forward` send a bare carriage return in `--body` (or in a quoted original) as a line break. It was written into the message as a lone CR, which RFC 5322 does not allow, so the line break was lost.
