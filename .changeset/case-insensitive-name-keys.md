---
"gws-rust": patch
---
`drive +sync`, `gmail +attachments` and `script +pull` now treat names that macOS and Windows file systems consider one file, such as `ας` and `ασ` (final and medial sigma) or `ſ` and `s`, as colliding, so one is disambiguated or refused instead of silently overwriting the other.
