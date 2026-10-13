---
"gws-rust": patch
---

Helpers that write straight to stdout now end quietly with exit status 0 when the reader closes the pipe (`gwsr ... | head`), like every other command. Previously `drive +download --output -`, `drive +export --output -`, `gmail +read`, `gmail +watch` and `events +subscribe` reported a broken pipe as an internal error (exit 5) on stderr.
