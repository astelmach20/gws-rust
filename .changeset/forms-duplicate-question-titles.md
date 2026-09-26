---
"gws-rust": patch
---

`forms +responses` no longer drops answers when two questions have the same title, which is common in multi-section forms, or when a question is titled like a fixed column such as `responseId`. The JSON rows are keyed by column header, so the later answer used to overwrite the earlier one. A repeated title now gets its question ID appended, for example `Comments (1a2b3c4d)`, in both the JSON and CSV headers.
