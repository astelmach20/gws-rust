---
"gws-rust": patch
---

A global option given both before and after a command or subcommand name (`gwsr --profile a auth --profile b list`, `gwsr --format json drive files list --format table`) is now rejected with "--profile was given more than once" (exit 3), like a repeat in one place. Previously the later value silently replaced the earlier one. Counted (`-v`) and list (`--columns`) options may still repeat.
