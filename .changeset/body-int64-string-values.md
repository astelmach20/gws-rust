---
"gws-rust": patch
---

`--json` request-body validation now checks the string value of an `int64` or `uint64` field: it must be a decimal integer in that format's range, as a JSON number for the field and the string form of an `int32` field already had to be. `"abc"`, `"1.5"` or `"-1"` for a `uint64` used to pass validation and be sent, and the API then rejected the request. A type error now also names the field's format (`Expected type 'string' with format 'int64', found string`).
