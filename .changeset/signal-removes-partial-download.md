---
"gws-rust": patch
---

A Ctrl-C (SIGINT) or SIGTERM while `gwsr` writes an output file (`-o PATH` downloads, `drive +download`, `+export`, Gmail attachments) no longer leaves a partial `.gwsr-output-*` temporary file next to the target. The partial file is removed and `gwsr` exits with 130 (SIGINT) or 143 (SIGTERM). Commands that already handle these signals themselves (`drive +sync`, `events +subscribe`, `gmail +watch`) keep doing so.
