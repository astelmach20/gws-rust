---
"gws-rust": patch
---

A multipart `--upload` (files up to 5 MiB) of a file that grows or shrinks while it is being sent now fails with a validation error (exit 3) naming the file, and nothing is uploaded. Previously a file that grew was cut off at the originally measured length, so the server received the new bytes with a truncated closing boundary, and a file that shrank failed with an opaque network error.
