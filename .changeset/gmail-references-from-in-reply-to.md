---
"gws-rust": patch
---

`gmail +reply`, `+reply-all` and `+forward` now follow RFC 5322 §3.6.4 when the original message has an `In-Reply-To` header but no `References` header: the new `References` header starts with that parent ID, followed by the original's Message-ID. Before, the parent was dropped and the chain held only the original's Message-ID, so clients that thread by `References` lost the earlier message.
