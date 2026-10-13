---
"gws-rust": patch
---

`gmail +reply-all` on a message you sent only to yourself now replies to you, as `+reply` does, instead of failing with "No To recipient remains". Anyone else on the original Cc stays in Cc, and `--remove` still takes precedence.
