---
"gws-rust": patch
---

Argument errors keep clap's suggestions in the JSON error envelope: a mistyped flag, resource or method now carries a `hint` such as `a similar argument exists: '--params'`, as the human (`--format table`) error already showed.
