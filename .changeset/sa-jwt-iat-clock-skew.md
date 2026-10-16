---
"gws-rust": patch
---

Service accounts: the signed JWT assertion's `iat` is now backdated by 10 seconds (with `exp` one hour after it), as Google's Go OAuth library does. A local clock running a few seconds ahead of Google's no longer gets the assertion rejected as issued in the future.
