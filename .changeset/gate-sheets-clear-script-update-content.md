---
"gws-rust": patch
---

The generated `sheets spreadsheets values clear`, `batchClear` and `batchClearByDataFilter` methods and `script projects updateContent` now need `--yes` (or a confirmed prompt) like other destructive methods. They erase data just as the already-gated `sheets +clear` and `script +push` helpers do, but ran without confirmation.
