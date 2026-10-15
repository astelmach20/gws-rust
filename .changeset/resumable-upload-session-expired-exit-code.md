---
"gws-rust": patch
---

A resumable upload whose session has expired (`404` or `410` on a chunk) now exits `1` (API error) with Google's error message and a hint to rerun the command to start a new upload. Previously it exited `5` (internal error).
