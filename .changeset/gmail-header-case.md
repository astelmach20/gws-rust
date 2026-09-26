---
"gws-rust": patch
---

Gmail `+reply`, `+reply-all` and `+forward` now read the original message's headers case-insensitively, as RFC 5322 requires. Messages whose sender wrote headers such as `CC:`, `Reply-to:` or `Message-id:` previously lost their Cc recipients on reply-all, ignored the Reply-To address, or failed with "Message is missing Message-ID header"; they are now handled like any other message.
