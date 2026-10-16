---
"gws-rust": patch
---

`script +push` now matches file extensions in any case. `Code.GS`, `Index.HTML` and `Lib.JS` used to be skipped without a word, and because a push replaces every file in the project, the matching remote file was deleted. They are now pushed as `Code`, `Index` and `Lib`. On a case-sensitive disk, `Code.gs` next to `Code.GS` is refused as two files mapping to one name.
