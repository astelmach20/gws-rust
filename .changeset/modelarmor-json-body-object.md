---
"gws-rust": patch
---

`modelarmor +create-template`, `+sanitize-prompt` and `+sanitize-response` now require `--json` to be a JSON object and reject repeated keys, as every other `--json` body does (exit 3). Before, `--json '[]'` or `--json '"x"'` was sent as the request body, and `{"a":1,"a":2}` silently sent only the last value.
