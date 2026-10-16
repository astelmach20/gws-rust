---
"gws-rust": patch
---

Helpers and workflows refuse an empty ID instead of sending a request to the wrong resource. Before, `calendar +delete --event-id ""` sent `DELETE .../events/` and reported `"deleted": true`, and `workflow +email-to-task --message-id ""` fetched `.../messages/` rather than one message. Any request URL whose path has an empty segment (`calendars//events`, or a trailing `/`) is now a validation error (exit 3), in `--dry-run` too, and nothing is sent.
