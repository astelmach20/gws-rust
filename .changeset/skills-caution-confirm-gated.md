---
"gws-rust": patch
---

Generated helper skills now carry the "write command, confirm with the user" caution for every helper that asks for confirmation (`--yes`), not only for a fixed list of names. `calendar +delete`, `+update` and `+rsvp`, `gmail +reply`, `+reply-all`, `+forward`, `+filter` and `+unsubscribe`, `sheets +clear`, `drive +share`, `script +run`, `admin +user-suspend` and `+group-add-member`, and `workflow +file-announce` were missing it.
