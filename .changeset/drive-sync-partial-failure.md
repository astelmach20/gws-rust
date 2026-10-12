---
"gws-rust": patch
---

`drive +sync` no longer stops at the first file the API refuses, such as a file whose owner disabled downloads or a Doc too large to export. Before, the whole sync aborted there: it printed nothing about the files it had already written, and every later run stopped at the same file. Now such files are listed under a new `"failed"` key (item id, local path and the API error), the rest of the folder is still mirrored, and the report is printed. The command then exits `1` with reason `syncPartialFailure`, the same way `gwsr batch` reports `batchPartialFailure`. Auth, network and local write errors still abort the sync.
