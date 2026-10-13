---
"gws-rust": patch
---

`drive +download --output -` and `drive +export --output -` no longer print a JSON summary after the raw payload on stdout, so piping the file (e.g. `| sha256sum`) gets exactly its bytes.
