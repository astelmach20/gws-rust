---
"gws-rust": patch
---

Request-body validation now range-checks `int32` and `uint32` fields given as JSON numbers, as it already did for their string form: `{"count": 5000000000}` for an `int32` field is refused before the request is sent instead of being rejected by the API. Fractions such as `1.0` stay refused in both forms.
