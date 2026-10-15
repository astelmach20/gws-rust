---
"gws-rust": patch
---

`--wait`: when polling a long-running operation fails (an `operations.get` error, a network failure or the wait timeout), the error now says that the request was accepted and names the operation it started, so a rerun no longer starts the operation a second time without warning. The error keeps its class and exit code. `--wait` on an API with no `operations.get` method is now refused before the request is sent, instead of after the operation has already started.
