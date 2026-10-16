---
"gws-rust": patch
---

`gmail +label`, `+filter create --add-label/--remove-label` now match label names case-insensitively for non-ASCII letters too, as documented: `--add 'follow-up/ünï'` finds the label `Follow-up/Ünï` instead of failing with "Unknown Gmail label(s)".
