---
"gws-rust": patch
---

Helper skills no longer split a tip that wraps over several help lines into one fragment bullet per line (for example "- a Model Armor failure sends nothing." in `gwsr-gmail-send`). Wrapped tips in helper `--help` now continue with a hanging indent, and the skills generator joins them into one bullet.
