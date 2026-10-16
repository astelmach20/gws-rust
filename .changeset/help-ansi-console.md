---
"gws-rust": patch
---

`--help` and `--version` no longer print raw ANSI escape codes (`←[1m`) on a Windows console that does not render them. Help output now makes the same color decision as error messages on stderr.
