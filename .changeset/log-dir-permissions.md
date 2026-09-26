---
"gws-rust": patch
---

`GWSR_LOG_FILE` / `log_file`: an existing log directory is now restricted to `0700`, as documented, instead of keeping looser permissions such as `0755`. A directory whose permissions cannot be set is an error.
