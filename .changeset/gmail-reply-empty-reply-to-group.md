---
"gws-rust": patch
---

`gmail +reply` and `+reply-all` now reply to the sender (From) when the original's `Reply-To` names no mailbox, for example only an empty group such as `undisclosed-recipients:;`. Previously they failed with "No To recipient remains after exclusions and --to additions".
