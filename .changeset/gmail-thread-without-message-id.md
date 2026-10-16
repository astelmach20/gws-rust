---
"gws-rust": patch
---

`gmail +reply`, `+reply-all` and `+forward` no longer fail with "Message is missing Message-ID header" (exit 5) when the original message has no `Message-ID`. As RFC 5322 §3.6.4 says, the new message is sent without `In-Reply-To`, and with `References` only when the original had a chain of its own. It stays in the original's Gmail thread, and a warning on stderr says that other mail clients may not thread it.
