---
"gws-rust": patch
---

`calendar +agenda --calendar-id ID` now finds calendars that are hidden in your calendar list. Before, it failed with "--calendar-id not found in your calendar list" because the calendar list omits hidden entries unless asked for them. Without `--calendar-id`, hidden calendars are still left out of the agenda.
