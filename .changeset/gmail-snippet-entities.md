---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` now decode the HTML entities in Gmail's snippet when a message has no text or HTML body and the snippet is used instead. Such messages read `Tom & Jerry's` rather than `Tom &amp; Jerry&#39;s`, and HTML replies and forwards no longer quote them double-escaped (`&amp;amp;`).
