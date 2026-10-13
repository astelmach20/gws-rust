---
"gws-rust": patch
---

`gmail +watch`, `events +subscribe` and `gmail +attachments` now write `--output-dir` files through the shared output writer: a symlink at a target name is replaced instead of followed, `GWSR_RESTRICT_PATHS` is checked per file, and attachments get the default output file mode.
