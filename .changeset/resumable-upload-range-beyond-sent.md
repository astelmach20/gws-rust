---
"gws-rust": patch
---

Resumable uploads now fail when the upload server's `308` reply, or its answer to a status query, claims more bytes than `gwsr` has sent. Before, `gwsr` skipped ahead to that offset and could report a finished upload while part of the file was never sent.
