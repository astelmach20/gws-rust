---
"gws-rust": patch
---

`-o/--output` naming an existing directory is now rejected before any request is sent, with a validation error (exit 3). Previously `gwsr` downloaded the whole response and then failed with an internal error (exit 5) when it tried to rename the file over the directory, or, for helpers without `--overwrite`, asked you to pass `--overwrite`, which could not help.
