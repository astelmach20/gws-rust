---
"gws-rust": patch
---

An API name that differs from the real one only in case (`gwsr siteverification:v1`, `gwsr SITEVERIFICATION:v1`) now fails as "No Discovery Document found" (exit 4) with the hint "did you mean `siteVerification:v1`? API names are case-sensitive", instead of an "Invalid Discovery Document" fetch failure. The per-service endpoint is now tried at the lowercase host for any API name, so `gwsr siteVerification:v1` loads. A missing API whose name matches a registered one ignoring case (e.g. `DRIVE:v3`) also suggests the registered name. Lookup itself stays case-sensitive.
