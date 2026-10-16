---
"gws-rust": patch
---

`gmail +watch` and `events +subscribe` now report the real number of failed attempts when they stop after `--max-failures`. With `--max-failures 2`, the error said "giving up after 2 consecutive transient failures" although three pulls had failed. It now says "giving up after 3 consecutive transient failures (--max-failures 2)".
