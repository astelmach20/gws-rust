---
"gws-rust": patch
---

`--jq` results containing `nan` or an infinity no longer fail as "not valid JSON". As in jq, `nan` prints as `null`, and `infinite` / `-infinite` print as the largest finite double with the same sign (`1.7976931348623157e308` / `-1.7976931348623157e308`). This applies at any depth inside arrays and objects.
