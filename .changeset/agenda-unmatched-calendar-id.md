---
"gws-rust": patch
---

`calendar +agenda --calendar-id` now fails (exit 3) when any requested calendar ID is not in your calendar list, and names the missing IDs. Before, an ID that matched nothing was dropped silently as long as another `--calendar-id` matched, so a typo quietly removed a calendar from the agenda.
