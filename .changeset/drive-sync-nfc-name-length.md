---
"gws-rust": patch
---

`drive +sync` caps the length of a local file name after Unicode normalization, not before. NFC can double a name's UTF-8 length (U+0958 and other composition exclusions decompose), so a long Drive name could become a local name over the 255-byte limit of ext4, and the sync aborted when it could not create the file.
