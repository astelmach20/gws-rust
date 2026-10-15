---
"gws-rust": patch
---
When a retried DELETE gets 404 or 410 after an earlier attempt timed out or returned a server error, the error now says that the earlier attempt may already have deleted the resource.
