---
"gws-rust": patch
---

`--json` and `gwsr batch` bodies accept a JSON integer for a 64-bit integer field (Discovery `"type": "string"` with `"format": "int64"` or `"uint64"`), as the APIs do. Before, `{"size": 5}` failed validation with "Expected type 'string', found integer". Fractions and values outside the format's range are still rejected.
