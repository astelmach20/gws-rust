---
"gws-rust": patch
---

`gmail +read`, `+reply`, `+reply-all`, `+forward` and `+attachments` now fetch a large message body that the Gmail API returns by attachment ID instead of inline. Before, such a body was treated as a file: `+read` printed the truncated API snippet, replies and forwards quoted only the snippet, `+forward` attached the body as `part-0.html`, and `+attachments` saved it as one. If the body can't be fetched, the command fails with the API error instead of falling back to the snippet.
