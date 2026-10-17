---
"gws-rust": patch
---

`gwsr auth` help (`gwsr auth`, `gwsr auth <command> --help`, `gwsr help auth ...`) piped into a reader that closes early, such as `| head`, now exits 0 quietly like every other help page, instead of printing a `failed to print help: Broken pipe` validation error and exiting 3.
