---
"gws-rust": patch
---

A token-endpoint error no longer repeats access, refresh or ID tokens from the response body. When Google's token endpoint (or a proxy in front of it) returned a token response that failed to parse, for example with `expires_in` as a string, the error on stderr quoted the body, live tokens included. In a JSON error body the values of token, secret and assertion fields are now shown as `[redacted]`. The rest of the body is still quoted.
