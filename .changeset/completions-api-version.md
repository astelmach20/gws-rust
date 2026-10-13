---
"gws-rust": patch
---

Shell completions now follow the API version you select. `gwsr drive --api-version v2 <TAB>` completes the resources and methods of the cached Drive v2 document instead of the default v3, and `gwsr drive:v2 <TAB>` (or any `<api>:<version>`) completes at all, from the same document the command would load. An invalid version completes nothing instead of falling back to the default version.
