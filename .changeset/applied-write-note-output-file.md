---
"gws-rust": patch
---

When a generated method that changes something (a non-GET request) succeeds but its result cannot be saved with `-o` (for example because of a bad `--decode-field`), the error now says that the change was made and names the resource. The same applies when `--jq` or `--format` fails on the saved-file summary, as already happens for printed results.
