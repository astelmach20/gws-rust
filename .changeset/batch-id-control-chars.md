---
"gws-rust": patch
---

`gwsr batch` rejects call ids containing control characters (ESC, NUL, tab, DEL) or invisible/bidi Unicode, which previously went raw into the MIME `Content-ID` header and the terminal confirmation prompt.
