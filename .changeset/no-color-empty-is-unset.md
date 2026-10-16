---
"gws-rust": patch
---

An empty `NO_COLOR=` no longer turns color off. Following no-color.org, `NO_COLOR` disables color only when it is set to a non-empty value. This applies to stderr messages and to `--help` output.
