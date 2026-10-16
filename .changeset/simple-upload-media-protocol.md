---
"gws-rust": patch
---
`--upload` on a method whose simple upload is documented with `multipart: false` now sends `uploadType=media` (the raw file with its content type), and with `--json` metadata uses resumable when available or fails with a validation error before sending.
