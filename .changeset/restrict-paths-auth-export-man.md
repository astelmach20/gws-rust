---
"gws-rust": patch
---

`GWSR_RESTRICT_PATHS=cwd` now also confines `dev man --output-dir` and `auth export --unmasked --output`, which could previously write outside the current directory.
