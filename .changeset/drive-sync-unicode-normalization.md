---
"gws-rust": patch
---

`drive +sync` no longer loses a file when two Drive names in one folder differ only by Unicode normalization (for example a precomposed `café.txt` and a decomposed `café.txt`). macOS file systems treat them as the same file, so the second was reported `unchanged` and never downloaded. Local names are now NFC-normalized, and the second item is saved with its ID prefix like other same-name items.
