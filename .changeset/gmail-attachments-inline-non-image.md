---
"gws-rust": patch
---

`gmail +attachments` no longer skips files such as PDFs that the sender marked inline with a Content-ID (as Apple Mail does); only inline images need `--include-inline`.
