---
"gws-rust": patch
---

`--format table --columns` on a single object (a `get` response or `--dry-run` output) lists the fields in the order given, as CSV and multi-row tables already do, instead of in the response's key order.
