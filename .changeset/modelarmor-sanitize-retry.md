---
"gws-rust": patch
---

`modelarmor +sanitize-prompt` and `+sanitize-response` now retry a timeout or a 5xx response like any other idempotent call. They used to fail at once, although sanitizing only screens text and changes nothing on the server. The `--sanitize` path already retried. `+create-template` is still never re-sent once the request may have reached the server.
