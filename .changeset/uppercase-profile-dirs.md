---
"gws-rust": patch
---

`auth list` and `auth status` no longer silently hide profile directories whose names are not valid profile names (such as uppercase names from an older gwsr): they are reported under `invalid_profiles` with a warning, and `--profile Work` explains how to rename the directory so it can be used or logged out of.
