---
"gws-rust": patch
---

`--json` bodies are no longer rejected for keys inside map-typed schemas such as Admin Directory `customSchemas` (`UserCustomProperties`); only the values are type-checked.
