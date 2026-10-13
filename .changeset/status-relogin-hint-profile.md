---
"gws-rust": patch
---

`gwsr auth status` and `gwsr auth export` now name the active profile in their "sign in again" hint (`gwsr auth login --profile <name>`), as API commands already did, instead of always suggesting plain `gwsr auth login`, which signs in to a different profile when `--profile` selected this one.
