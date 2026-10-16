---
"gws-rust": patch
---

`gmail +read`, `+reply`, `+reply-all` and `+forward` now find a message body whose part type is written in upper or mixed case, such as `TEXT/PLAIN` or `Text/Html`. MIME types are case-insensitive, but these parts were skipped, so the body fell back to the truncated API snippet.
