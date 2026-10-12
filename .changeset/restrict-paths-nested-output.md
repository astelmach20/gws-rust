---
"gws-rust": patch
---

`GWSR_RESTRICT_PATHS=cwd` now also confines files written *under* an output directory. `script +pull` and `drive +sync` validated `--dir` itself, but the files they created below it (named after remote files and folders) could pass through a symlinked sub-directory and land outside the current directory. Every output file is now checked before it is written.
