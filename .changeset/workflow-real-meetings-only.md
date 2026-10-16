---
"gws-rust": patch
---

`workflow +standup-report` and `+weekly-digest` count only real meetings: they leave out events you declined, cancelled events, and working-location, out-of-office, focus-time, birthday and other non-`default` calendar entries, which they used to list and count. `+meeting-prep` applies the same filter, so it no longer picks a timed focus-time or out-of-office block as your next meeting.
