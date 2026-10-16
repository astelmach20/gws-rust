---
"gws-rust": patch
---

`--jq` no longer offers jq's `stderr`, `debug`, `debug(msg)` and `halt_error` builtins. They wrote their input to stderr as is, bypassing gwsr's terminal sanitization, so a filter such as `--jq '.[].subject | stderr'` could send escape sequences from API data (a message subject, a file name) straight to the terminal. A filter that uses them is now rejected before any request (exit 3). `input` and `inputs` remain unavailable.
