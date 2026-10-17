---
"gws-rust": patch
---

`--json` request-body validation now range-checks `float` fields. A value beyond the 32-bit float range (`1e39`, or the string `"1e39"`) used to pass validation and be sent, and the API's JSON parser then rejected it. `double` fields still take any finite number.
