---
"gws-rust": patch
---

`gwsr docs +create --markdown` no longer creates an empty document before rejecting unsupported Markdown (tables, images, HTML...). The Markdown, `+read --output` (including `--body-format raw` with `--output`) and an empty `+replace --find` are now checked before credentials are loaded, so they exit with a validation error (3) instead of an authentication error (2).
