---
"gws-rust": patch
---

`script +pull` refuses a project whose file names differ only in case (`Code` and `code`, or `lib/a` and `Lib/b`), and writes nothing. On a case-insensitive disk (the macOS and Windows default), one file used to replace the other while both were reported as written, even with `--overwrite`. Rename one of them in the Apps Script editor.
