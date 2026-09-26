---
"gws-rust": patch
---

A failed long-running operation (`--wait`, `events +subscribe`) now reports the HTTP status for its gRPC error code, and names the gRPC status in the message. Previously the raw gRPC code was reported as if it were an HTTP status (`"code": 14`), so a transient `UNAVAILABLE` or `RESOURCE_EXHAUSTED` failure exited with `1` ("don't retry") instead of `6`.
