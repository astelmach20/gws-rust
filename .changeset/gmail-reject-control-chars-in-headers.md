---
"gws-rust": patch
---

`gmail +send`, `+reply`, `+reply-all` and `+forward` now reject a `--subject`, `--to`, `--cc`, `--bcc` or `--from` value that contains a control character (CR/LF, tab, NUL and the like) or invisible/bidi Unicode, with a validation error (exit 3) before any request. Previously the subject was sent with the line break encoded into it, and control characters in addresses and display names were dropped without a word.
