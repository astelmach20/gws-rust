---
"gws-rust": patch
---

`--json` request-body validation now accepts the string forms the proto3 JSON mapping allows: integers as strings (`"5"`, range-checked for `int32` and `uint32`) and floating-point numbers as strings (`"1.5"`, `"NaN"`, `"Infinity"`, `"-Infinity"`). Google APIs accept these bodies, but `gwsr` used to refuse them before sending.
