---
"gws-rust": patch
---

`gwsr batch classroom` now sends at most 50 calls per batch request, the Classroom API's documented limit. With 51 to 100 calls it used to send them in one request, which Classroom rejects as a whole. Other APIs still use groups of up to 100, and results stay in input order.
