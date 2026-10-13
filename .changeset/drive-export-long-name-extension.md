---
"gws-rust": patch
---

`drive +export` without `--output` keeps the format's extension (e.g. `.pdf`) when it shortens a long Drive file name for the local file, instead of cutting the extension off.
