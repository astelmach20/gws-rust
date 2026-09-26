---
"gws-rust": patch
---

`drive +sync` no longer drops a file whose name differs from a sibling only by letter case (for example `Report.txt` and `report.txt`). On case-insensitive file systems (the macOS and Windows defaults) both mapped to one local file, so the second was silently reported as `unchanged` or overwrote the first. Such names now count as a collision, so each is saved as `<file id>-<name>`, like other same-name siblings.
