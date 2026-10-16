---
"gws-rust": patch
---

`gmail +send`, `+reply`, `+reply-all` and `+forward` no longer base64-encode a `message/rfc822` attachment (an attached `.eml`, or an attached message carried over by `+forward`), which RFC 2046 §5.2.1 forbids. A message with CRLF line endings, lines of at most 998 octets and no NUL is sent unencoded (`7bit`, or `8bit` when it is UTF-8). Any other file is sent byte for byte as `application/octet-stream` under the same name.
