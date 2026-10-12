---
"gws-rust": patch
---

`gmail +label`, `+archive`, `+trash`, `+filter list`/`delete` and `+resolve-url` now apply `--sanitize` (and `GWSR_SANITIZE_TEMPLATE`) to their output. They used to ignore it silently and print unscreened results even in `block` mode.

`gmail +send`, `+reply`, `+reply-all` and `+forward` (also with `--draft`) now screen the outgoing subject and body, including quoted or forwarded text, with Model Armor **before** sending. In `block` mode a match (exit `11`) or a failure to reach Model Armor (the cause's exit code) sends nothing; in `warn` mode the message is sent after a warning and the printed result carries `_sanitization`. `+filter create` screens the filter before creating it and `+unsubscribe` screens the unsubscribe URL before the one-click POST, with the same rules. `--dry-run` does not call Model Armor.
