---
"gws-rust": patch
---

`gmail +attachments` no longer gives a long attachment name a trailing dot or space when it shortens it to 200 bytes; Windows drops them, so the file ended up with a different name than the one reported.
