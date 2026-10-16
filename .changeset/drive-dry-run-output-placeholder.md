---
"gws-rust": patch
---

`drive +download --dry-run` and `drive +export --dry-run` without `--output` now show the local file as `<file name>` (or `<file name>.pdf` and so on) in the current directory, since a real run names it after the Drive file. They used to plan a made-up `download` or `export.pdf`, and failed with "--output '…/download' is a directory" when a directory of that name existed, though no `--output` was given.
