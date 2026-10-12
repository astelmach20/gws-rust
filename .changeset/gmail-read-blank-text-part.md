---
"gws-rust": patch
---

Gmail `+read`, `+reply`, `+reply-all` and `+forward` no longer treat a whitespace-only `text/plain` alternative as the message body when the message also has an HTML part: the HTML part is rendered as text instead (links kept as footnotes), so `+read` no longer prints an empty body for such messages. When a message has no text or HTML body part at all, the fallback to the API snippet (which Gmail truncates) now logs a warning on stderr.
