---
name: recipe-batch-invite-to-event
description: "Add a list of attendees to an existing Google Calendar event and send notifications."
metadata:
  version: 0.23.0
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-calendar
---
<!-- gwsr generated skill: do not edit by hand -->

# Add Multiple Attendees to a Calendar Event

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-calendar` (install any that are missing with `npx skills add https://github.com/astelmach20/gws-rust/tree/main/skills/<name>`)

Add a list of attendees to an existing Google Calendar event and send notifications.

## Steps

1. Get the event: `gwsr calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
2. Add attendees: `gwsr calendar events patch --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}' --json '{"attendees": [{"email": "alice@company.com"}, {"email": "bob@company.com"}, {"email": "carol@company.com"}]}'`
3. Verify attendees: `gwsr calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`

