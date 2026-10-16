---
"gws-rust": patch
---

`events +subscribe` and `gmail +watch` no longer treat a Pub/Sub pull whose connection timed out as "no messages yet". A connect timeout means the request was never sent, so it now counts as a transient network failure (bounded by the stream's failure budget) instead of being silently retried forever.
