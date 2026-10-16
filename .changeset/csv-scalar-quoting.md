---
"gws-rust": patch
---

`--format csv` now quotes a lone scalar value (a `--jq` result such as `--jq .name`, or a string item of `--page-items`) like any other CSV field. Previously it was printed raw, so a value with a comma, a double quote or a line break split into several fields or records.
