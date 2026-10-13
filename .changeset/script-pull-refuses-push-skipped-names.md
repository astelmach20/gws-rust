---
"gws-rust": patch
---

`script +pull` refuses project files that `script +push` would skip: names with a part starting with `.` (such as `.config/settings`) or a `node_modules` directory. Before, `+pull` wrote them, the next `+push` of the same directory silently left them out, and because `+push` replaces every file in the project, those files were deleted from the project. Nothing is written when the project has such a file; rename it in the Apps Script editor.
