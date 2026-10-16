---
"gws-rust": patch
---

`gmail +forward` no longer drops a PDF or other non-image file that the original sender marked `Content-Disposition: inline` with a Content-ID (as Apple Mail does) from a plain-text forward, and `--no-original-attachments` now leaves it out like any other attachment. In an HTML forward such a file is a regular attachment instead of a part beside the HTML body, and `+reply --html` / `+reply-all --html` no longer re-attach it to the reply. Only inline images count as inline, as for `+attachments`.
