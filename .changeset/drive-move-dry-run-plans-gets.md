---
"gws-rust": patch
---

`drive +move --dry-run` now lists the two `files.get` reads that a real run sends before its PATCH: one for the file's current parents and one for the destination folder's real ID. The dry run still sends no request.
