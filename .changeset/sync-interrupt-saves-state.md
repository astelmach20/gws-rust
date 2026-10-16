---
"gws-rust": patch
---
`drive +sync` stopped by Ctrl-C or SIGTERM now saves the sync state of the files it finished and exits 130 (SIGINT) or 143 (SIGTERM), so the next run no longer reports those files as conflicts.
