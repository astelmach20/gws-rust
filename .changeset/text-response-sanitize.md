---
"gws-rust": patch
---

`--sanitize` now screens text responses of generated methods (for example `drive files export` to `text/plain`, or a text file fetched with `alt=media`) that are printed inline as JSON. Previously only JSON responses were screened, so text content reached stdout unscreened even in block mode. Bodies saved with `-o PATH` or streamed with `-o -` are unchanged.
