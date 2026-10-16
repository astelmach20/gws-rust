---
"gws-rust": patch
---

`--format table` now keeps a key that contains a newline or tab on one line in the header and in key/value output, as it already did for values. Two keys that display the same after control characters are stripped now each show their own values.
