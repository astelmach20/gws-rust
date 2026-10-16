---
"gws-rust": patch
---

`gmail +reply` and `+reply-all` now treat every send-as alias as you, not just the primary address and `--from`. `+reply-all` no longer copies your own alias on a reply to a message you received at that alias. A reply to a message you sent from an alias now goes to its original recipients instead of back to you. Both commands now also read the account's send-as settings when `--from` is not given.
