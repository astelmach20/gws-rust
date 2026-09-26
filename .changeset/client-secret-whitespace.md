---
"gws-rust": patch
---

A `client_secret.json` whose `client_id` or `client_secret` is empty or contains whitespace, control or non-ASCII characters (typically a space pasted with the value) is now rejected with an error naming the field and file, instead of being sent to Google and failing as `invalid_client` ("The OAuth client was not found"). The same values are refused when the file is written.
