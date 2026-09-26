---
"gws-rust": patch
---

`workflow +standup-report`, `+weekly-digest`, `+email-to-task` and `+file-announce` `--dry-run` plans now list every query parameter the real run sends (the calendar time window, `singleEvents`, `orderBy`, `maxResults`, `metadataHeaders`, `fields`, `supportsAllDrives` and the Chat `requestId`), with `<placeholders>` for values only known at run time. Previously they left these out, so the plan did not describe the requests that would be sent.
