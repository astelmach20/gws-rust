---
name: recipe-share-event-materials
description: "Share Google Drive files with all attendees of a Google Calendar event."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-calendar
        - gwsr-drive
---
<!-- gwsr generated skill: do not edit by hand -->

# Share Files with Meeting Attendees

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-calendar`, `gwsr-drive` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Share Google Drive files with all attendees of a Google Calendar event.

## Steps

1. Get event attendees: `gwsr calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
2. Share file with each attendee: `gwsr drive permissions create --params '{"fileId": "FILE_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "attendee@company.com"}'`
3. Verify sharing: `gwsr drive permissions list --params '{"fileId": "FILE_ID"}' --format table`

