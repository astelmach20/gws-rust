---
"gws-rust": patch
---

`gwsr workflow +standup-report` and `+weekly-digest` no longer hang when the API repeats a `nextPageToken`, which now fails with an error, or returns an empty one, which ends pagination.
