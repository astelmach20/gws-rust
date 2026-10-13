---
"gws-rust": patch
---

`gwsr chat +send` now rejects an empty `--text` before asking for confirmation or loading credentials, so it exits with a validation error (3) instead of a confirmation (7) or authentication (2) error.
