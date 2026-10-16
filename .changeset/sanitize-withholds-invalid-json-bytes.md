---
"gws-rust": patch
---

With `--sanitize` set, a JSON response that fails to parse no longer prints its first 200 bytes, unscreened, in the error on stderr. In either mode the error now says that the content was withheld. Without `--sanitize`, the error still quotes the first bytes.
