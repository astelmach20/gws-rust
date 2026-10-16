---
"gws-rust": patch
---

`gmail +unsubscribe` no longer prints the one-click URL's path and query in its network error (exit 10) when the sender's server cannot be reached. Unsubscribe URLs often carry a per-recipient token; the error now names the host only, as the main HTTP client's errors do.
