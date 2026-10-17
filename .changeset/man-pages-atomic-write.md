---
"gws-rust": patch
---

`gwsr dev man` writes each page atomically and replaces a symlink at a page's path instead of writing through it, so a symlink in `--output-dir` can no longer overwrite the file it points to (or, with `GWSR_RESTRICT_PATHS=cwd`, reach outside the current directory: that is now refused). Every page is rendered before any is written.
