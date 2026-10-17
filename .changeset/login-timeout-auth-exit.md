---
"gws-rust": patch
---

`gwsr auth login` now exits 2 (`authError`) when the browser sign-in times out, instead of 8 (`configError`). A timed-out sign-in leaves the profile without credentials; nothing about the configuration is wrong.
