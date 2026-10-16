---
"gws-rust": patch
---
A `Retry-After` with more seconds than fit in 64 bits now counts as a very long wait, so gwsr stops retrying and returns the response, instead of ignoring the header and retrying on its own short backoff; a signed value such as `+5` is ignored as invalid.
