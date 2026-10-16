---
"gws-rust": patch
---

`gmail +read`, `+reply`, `+reply-all` and `+forward` now keep the whole body of a message whose text is split into several parts around inline attachments, as Apple Mail sends it. Only the first `text/html` (or `text/plain`) part was used, so everything after the first inline attachment was missing from the output and from the quoted original. The alternatives of `multipart/alternative` are still never joined, and the text of an embedded `message/rfc822` message or of a text part marked `Content-Disposition: attachment` is not added to the body.
