---
"gws-rust": patch
---

The error for an unknown `GWSR_*` environment variable no longer suggests running `gwsr --help` instead of unsetting it. The variable is checked before any command runs, so `gwsr --help` failed with the same error. The message now says to unset the variable first, and that `gwsr --help` then lists the supported variables.
