---
"gws-rust": patch
---

`gmail +reply`, `+reply-all` and `+forward` now parse the original's `References` and `Message-ID` headers as RFC 5322 message-ID lists. `<a@x><b@x>` (no space between IDs) was read as one ID, and a `Message-ID` followed by a comment such as `<id@x> (added by relay)` produced a malformed `In-Reply-To: <<id@x> (added by relay)>`, which breaks threading in the recipients' mail clients (debug builds panicked on both).
