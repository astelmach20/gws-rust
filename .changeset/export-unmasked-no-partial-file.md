---
"gws-rust": patch
---

`gwsr auth export --unmasked --output FILE` no longer leaves a half-written secrets file behind when a write or fsync fails. The credentials are written to a private temporary file in the same directory and moved into place only after they are fully written and synced, so a retry is not refused because of the partial file. Such a failure is now reported as an I/O error (exit 5) instead of a validation error (exit 3); an existing FILE is still refused with exit 3.
