---
"gws-rust": patch
---

`gwsr dev generate-skills --index` refuses a directory (an existing one, or a path ending in `/`) with a validation error (exit 3) before writing any skill. It used to write every skill and then fail with an internal error (exit 5) when writing the index.
