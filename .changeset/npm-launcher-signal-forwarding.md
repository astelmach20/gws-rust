---
"gws-rust": patch
---

npm launcher: `SIGTERM`, `SIGINT`, `SIGHUP` and `SIGQUIT` sent to the `gwsr` launcher are now forwarded to the native binary, and the launcher waits for its real exit status. Previously the launcher died on its own and left `gwsr` running as an orphan (for example under `docker stop`, Kubernetes or `kill <pid>`), which skipped the clean shutdown of `+watch`/`+subscribe` streams. When the binary dies from a signal Node cannot re-raise (such as `SIGPIPE`), the launcher now exits with `128 + signal number` instead of `1`.
