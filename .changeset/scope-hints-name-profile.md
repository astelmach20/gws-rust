---
"gws-rust": patch
---

Scope and sign-in hints now name the active profile. With `--profile work` (or `GWSR_PROFILE`, or a non-default profile), the `hint` of an auth error or a missing-scope 403, the "refused the requested scopes" error and the scope warning suggest `gwsr auth login --profile work --scopes ...` instead of plain `gwsr auth login --scopes ...`, which signed in to the config/default profile and could replace that profile's login.
