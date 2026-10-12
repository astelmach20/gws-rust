---
"gws-rust": patch
---

`calendar +agenda --dry-run` now shows the `maxResults=250` query parameter on its `calendarList` request, matching the request the command actually sends.
