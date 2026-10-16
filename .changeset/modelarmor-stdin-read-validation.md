---
"gws-rust": patch
---
`modelarmor +sanitize-prompt` and `+sanitize-response` now exit 3 (validation error) instead of 5 (internal error) when the text piped on stdin can't be read, for example when it isn't valid UTF-8, matching every other flag that reads stdin.
