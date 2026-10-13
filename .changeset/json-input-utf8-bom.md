---
"gws-rust": patch
---

JSON read from a file or stdin (`--params`/`--json` with `@file` or `-`, and `gwsr batch` input) may now start with a UTF-8 byte order mark, as files saved by Windows editors and Windows PowerShell do. Previously such input was rejected with `expected value at line 1 column 1` (exit 3).
