---
"gws-rust": patch
---

`--sanitize` now screens the result that `--wait` prints. Previously only the pending long-running operation was sent to Model Armor, and the finished operation's response was printed (or saved with `-o`) unscreened, even in `block` mode.
