---
"gws-rust": patch
---

Table and CSV output no longer drop a value when a field name contains a dot. A literal `"a.b"` key and a nested `{"a": {"b": …}}` used to flatten to the same `a.b` column, and one of the two values was silently lost. Dots and backslashes inside field names are now escaped in column names (`a\.b`, `a\\b`), and `--columns` accepts the escaped form. Column headers for such fields change accordingly.
