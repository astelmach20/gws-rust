---
"gws-rust": patch
---

`gwsr batch` now honors `--sanitize` / `GWSR_SANITIZE_TEMPLATE`: every result body is screened by Model Armor before it is printed, and block mode fails closed exactly as for a single call. Previously batch ignored the setting and printed API content unscreened, even in block mode.
