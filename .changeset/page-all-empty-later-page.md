---
"gws-rust": patch
---

With `--page-all`/`--page-items`, a later page that comes back empty (204 or no body) now ends the stream with a warning on stderr instead of printing a `{"status":"success"}` object into the NDJSON output.
