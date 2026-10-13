---
"gws-rust": patch
---

Refuse request URLs that hide a `.` or `..` path segment behind a backslash or an ASCII tab/CR/LF (for example `files/F1/..\x`), which URL parsing would otherwise silently remove, retargeting the request.
