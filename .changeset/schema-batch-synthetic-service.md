---
"gws-rust": patch
---

`gwsr schema workflow...` and `gwsr batch workflow` now fail at once with a validation error (exit 3) that says `workflow` has no Discovery document and points to `gwsr workflow --help`. They used to ask the network for a document that does not exist and failed with a misleading Discovery error (exit 4).
