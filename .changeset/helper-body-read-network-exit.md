---
"gws-rust": patch
---

Helper commands (`+verb`): a connection that drops or stalls while the response body is being read now exits `10` (`networkError`), as generated API methods already did. Previously the helper REST client reported it as an internal error (exit `5`).
