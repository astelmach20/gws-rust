---
"gws-rust": patch
---

Refuse requests whose URL path would contain a `.` or `..` segment (an ID or resource name of `.` or `..`, literal or percent-encoded) with a validation error (exit 3), in `--dry-run` too. URL parsing removed such segments, so the request addressed a different resource: `gwsr calendar +delete --calendar-id CAL --event-id ..` sent `DELETE …/calendars/CAL/` (the calendar, not an event), and `drive permissions delete` with `"permissionId": ".."` sent `DELETE …/files/FILE_ID/`.
