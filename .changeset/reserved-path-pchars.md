---
"gws-rust": patch
---

`{+name}` path parameters now accept documented resource names that contain `@`, `:` or other RFC 3986 path characters, such as Chat's `users/user@example.com/...` and `customEmojis/:emoji-name:`. These characters are percent-encoded as needed; `%`, `?`, `#`, whitespace and `.`/`..` segments are still refused.
