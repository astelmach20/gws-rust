---
"gws-rust": patch
---

`gmail +label` and `+archive` split more than 1000 message IDs into several `batchModify` requests (Gmail's per-request limit) instead of failing with HTTP 400.
