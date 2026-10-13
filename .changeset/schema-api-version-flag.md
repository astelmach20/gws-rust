---
"gws-rust": patch
---

`gwsr schema` now honors the global `--api-version` flag, which wins over a `:version` suffix in the path, as it does when the method runs. Previously the flag was accepted but ignored, so `gwsr schema drive.files.list --api-version v2` printed the v3 schema.
