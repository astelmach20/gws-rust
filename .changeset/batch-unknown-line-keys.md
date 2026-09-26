---
"gws-rust": patch
---

`gwsr batch` now rejects input lines with keys other than `id`, `method`, `params` and `json` (for example a misspelled `body` or `parms`), `json` on a method that takes no request body, and `upload` on any method. Previously those keys were silently dropped and the call was sent without them.
