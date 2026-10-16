---
"gws-rust": patch
---

Gmail helpers no longer download an unnamed text attachment as if it were the message body when its `Content-Disposition` starts with whitespace (` attachment`). The part keeps its attachment ID and is listed as a file, as it already was without the leading space.
