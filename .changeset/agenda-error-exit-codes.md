---
"gws-rust": patch
---

`calendar +agenda` no longer reports every per-calendar failure as an internal error (exit 5). A calendar that cannot be read now keeps its documented exit code (1 for API errors, 2 for auth, 6 for 429/5xx rate limits, 10 for network errors), with the calendar name still in the message.
