---
"gws-rust": patch
---

Methods that a Discovery document flags with `deprecated: true` (for example `admin chromeosdevices action`) are now hidden from help and completions, as the README describes. Only methods whose description starts with "Deprecated" were hidden before. Hidden methods still run when called by name.
