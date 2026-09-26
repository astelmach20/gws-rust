---
"gws-rust": patch
---

`workflow` helpers (`+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`) now honor `--sanitize` / `GWSR_SANITIZE_TEMPLATE`. Previously they ignored the Model Armor configuration and printed calendar, Gmail and Drive content unsanitized, even in `block` mode.
