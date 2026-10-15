---
"gws-rust": patch
---
An invalid Discovery cache entry that cannot be deleted no longer fails the command: it is ignored with a warning and the document is fetched again.
