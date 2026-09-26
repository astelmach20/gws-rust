---
"gws-rust": patch
---

Gmail helpers that take a single `--message-id` (`+read`, `+reply`, `+reply-all`, `+forward`, `+download`, `+unsubscribe`) now reject a value that is not a Gmail API message ID (for example `<ID>` or an RFC `Message-ID` header such as `<abc@mail.example.com>`) with a validation error (exit 3). Previously `+reply`, `+reply-all` and `+forward --dry-run` crashed on such a value in debug builds and printed a malformed `In-Reply-To` header in release builds. Threading headers are also checked before a message is built, so an ID containing angle brackets, white space or control characters is an error instead of a broken header.
