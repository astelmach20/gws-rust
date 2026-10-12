---
"gws-rust": patch
---

`gmail +reply-all` and `+reply` now understand RFC 5322 group syntax in the original message's To, Cc and Reply-To headers. Replying all to a message addressed to `undisclosed-recipients:;` (any Bcc-only message) no longer adds a bogus `<undisclosed-recipients:;>` Cc recipient, and members of a named group such as `Team: a@example.com, b@example.com;` keep their correct addresses instead of being mangled into `Team: a@example.com` and `b@example.com;`.
