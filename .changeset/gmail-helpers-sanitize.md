---
"gws-rust": patch
---

`gmail +send`, `+reply`, `+reply-all`, `+forward`, `+label`, `+archive`, `+trash`, `+filter`, `+unsubscribe` and `+resolve-url` now apply `--sanitize` (and `GWSR_SANITIZE_TEMPLATE`) to their output. They used to ignore it silently and print unscreened results even in `block` mode.
