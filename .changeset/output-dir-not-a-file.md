---
"gws-rust": patch
---

An `--output-dir` (or `+sync --dir`) that names an existing file is now refused up front with a validation error (exit 3), instead of failing with an internal error (exit 5) part-way through the command.
