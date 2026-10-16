---
"gws-rust": patch
---

The `gmail +watch` and `events +subscribe` help tips, their skills and the README now describe `--max-failures N` correctly: the stream tolerates N consecutive transient failures and exits on the next one. They used to say it exits after N failures.
