---
"gws-rust": patch
---

`gwsr batch` now rejects a call whose `params` set `alt` to anything other than `json` (for example `alt=media` to download a Drive file, or `alt=proto`). The error names the input line. Batch results are decoded as JSON text, and Google does not serve media downloads in a batch, so such a call used to be sent and its result printed as a mangled string. Download media with a separate command instead.
