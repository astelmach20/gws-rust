---
"gws-rust": patch
---
Idempotent requests are now retried when the connection is reset or closed after the request was sent, as documented for connection errors, instead of failing on the first attempt.
