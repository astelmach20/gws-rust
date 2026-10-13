---
"gws-rust": patch
---

`--wait` now keeps polling until `--wait-timeout` is actually reached. It used to give up as soon as the next poll interval would end past the deadline, so `--wait-timeout 1` failed without polling even once, and longer timeouts stopped up to 10 seconds early.
