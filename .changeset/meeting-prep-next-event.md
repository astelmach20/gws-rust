---
"gws-rust": patch
---

`workflow +meeting-prep` now shows the next meeting that has not started yet. It used to return the first event whose end was after now, so an all-day event (such as a working-location or holiday entry), a meeting already in progress, or an event you declined was reported as your next meeting.
