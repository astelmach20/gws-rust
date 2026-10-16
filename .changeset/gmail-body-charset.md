---
"gws-rust": patch
---

`gmail +read` (and the quoted original in `+reply`/`+forward`) now decodes a message body in the charset its `Content-Type` names. The Gmail API returns body bytes in their original charset, so ISO-8859-1, windows-1252, Shift_JIS and other non-UTF-8 bodies used to come out with U+FFFD in place of every non-ASCII character. A missing or US-ASCII charset is still read as UTF-8, and an unknown charset falls back to UTF-8 with a warning.
