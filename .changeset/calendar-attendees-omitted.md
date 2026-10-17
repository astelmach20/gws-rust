---
"gws-rust": patch
---

`calendar +rsvp` and `calendar +update` no longer risk removing guests from an event whose guest list is hidden from you (`attendeesOmitted`). In that case Google returns only your own entry, and writing that list back would uninvite everyone else. `+rsvp` now sends only your entry with `attendeesOmitted: true`, which tells the API to update just your response. `+update --add-attendee/--remove-attendee` now refuses such an event and sends nothing.
