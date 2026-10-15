---
"gws-rust": patch
---

Table and CSV output no longer drop a value when a field name is empty. A nested `{"": {"a": 1}}` used to flatten to the column `a`, the same as a sibling `"a"` field, and one of the two values was silently lost. An empty field name is now kept as a path segment, so the nested value is the column `.a`.
