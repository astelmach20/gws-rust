---
"gws-rust": patch
---

Gmail `+forward` passes on a meeting invitation sent as an unnamed `text/calendar` part, attached as `invite.ics`. Before, the part was neither the body nor an attachment, so it was dropped. `+attachments` and `+read` now list it too. An unnamed calendar part that Gmail returned by reference was named `part-N.calendar`; it is now also `invite.ics`. When the message also has a named `.ics` attachment with the same invitation, as Google Calendar sends it, only the named one is kept, so the invitation is not forwarded twice.
