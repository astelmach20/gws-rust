---
name: recipe-reschedule-meeting
description: "Move a Google Calendar event to a new time and automatically notify all attendees."
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

# Reschedule a Google Calendar Meeting

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-calendar`

Move a Google Calendar event to a new time and automatically notify all attendees.

## Steps

1. Find the event: `gwsr calendar +agenda`
2. Get event details: `gwsr calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
3. Update the time: `gwsr calendar events patch --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}' --json '{"start": {"dateTime": "2025-01-22T14:00:00", "timeZone": "America/New_York"}, "end": {"dateTime": "2025-01-22T15:00:00", "timeZone": "America/New_York"}}'`

