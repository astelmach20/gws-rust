---
"gws-rust": patch
---

`gmail +read`, `+attachments`, `+reply` and `+forward` no longer use a text part that isn't the message's body as the body. An unnamed text part marked `Content-Disposition: attachment` is now treated as an attachment. Before, it could replace the real (HTML) body and was missing from `+attachments`. The text of an embedded `message/rfc822` message is no longer read as the outer message's body. An unnamed `text/plain` attachment is saved as `part-N.txt`.
