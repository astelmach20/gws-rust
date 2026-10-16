---
"gws-rust": patch
---

`drive +upload --convert` now accepts every Office type Drive imports into Docs, Sheets or Slides, including macro-enabled, template and slideshow files (`.xlsm`, `.xltx`, `.docm`, `.dotx`, `.pptm`, `.potx`, `.ppsx` and so on), which it used to refuse with "no Google format accepts". A `--mime-type` given with different case or with parameters (`text/csv; charset=utf-8`) is matched like the plain type instead of being refused.
