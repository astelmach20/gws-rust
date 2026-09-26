---
"gws-rust": patch
---

`--params`, `--json` and `gwsr batch` input lines now reject JSON objects that repeat a key, such as `{"fileId":"a","fileId":"b"}`, with a validation error (exit `3`) that names the key. Previously the last value silently won, so an ambiguous argument could target a different resource than intended, for example in a delete.
