---
"gws-rust": patch
---

`drive +download --dry-run` and `drive +export --dry-run` now list the `files.get` metadata read that a real run sends before downloading or exporting the content. The dry run still sends no request.
