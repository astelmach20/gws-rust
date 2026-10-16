---
"gws-rust": patch
---

Tests only: every destructive helper (`sheets +clear`, `script +push`, `admin +user-suspend`, `gmail +filter delete`, `drive +share --role owner`, `calendar +delete`) is now checked end to end to refuse without `--yes`. Before, the `sheets +clear` test called the confirmation gate directly with a hard-coded impact and skipped itself on a terminal.
