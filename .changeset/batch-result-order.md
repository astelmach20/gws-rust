---
"gws-rust": patch
---

`gwsr batch` now prints results in input order even when Google returns the parts of a batch response in a different order, matching each part to its call by Content-ID. Duplicate call ids are rejected, including a generated id (a line's position) that equals an explicit id on another line. Previously both results were labeled with the same id.
