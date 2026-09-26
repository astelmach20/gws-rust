---
"gws-rust": patch
---

An API call whose access token is still rejected with HTTP 401 after the automatic refresh now fails as an auth error: exit code `2` with `"reason":"authError"`, as the exit-code table documents for expired or invalid credentials. It used to exit `1` ("API error, don't retry unchanged"), which pointed agents at the request instead of at `gwsr auth login`. The server's message and reason are kept in the error message.
