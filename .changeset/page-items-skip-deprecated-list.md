---
"gws-rust": patch
---

`--page-items` now skips a deprecated list field that sits next to its replacement, so Chat `spaces search` splits `results` into rows instead of failing with "several array fields (results, spaces)".
