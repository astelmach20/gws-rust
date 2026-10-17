---
"gws-rust": patch
---

`gwsr auth export --unmasked --output` refuses a path that names a directory (`out/`, `out/.` or an existing directory) with a validation error (exit 3), before any credentials are read. `--output out/` used to drop the trailing separator and write the unmasked secrets to a new file named `out`.
