---
"gws-rust": patch
---

`gmail +attachments` refuses an attachment whose target in `--output-dir` is an existing directory before downloading anything, with or without `--overwrite`. Before, it wrote the attachments listed before that one and then failed part-way.
