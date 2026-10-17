---
"gws-rust": patch
---

`tasks +list` and `workflow +standup-report` now include tasks assigned to you from Google Docs or Chat. The Tasks API leaves these out unless `showAssigned` is set, so both commands silently dropped them before.
