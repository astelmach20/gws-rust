---
"gws-rust": patch
---

`calendar +insert`, `+update` and `+freebusy` no longer crash on a huge `--duration` or `--slot` (for example `99999999999999999d`, or a length that ends past the latest representable date). They now fail with a validation error (exit 3).
