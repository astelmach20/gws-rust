---
"gws-rust": patch
---

When a request is retried, then gets HTTP 401 and is sent again with a refreshed token, its error now describes the whole request. "gave up after N attempts" counts the attempts made before the refresh. A DELETE that finds nothing (404/410) after an earlier attempt may have been applied (a timeout or a 5xx) still says that earlier attempt may already have deleted it. Before, the refresh threw away that history.
