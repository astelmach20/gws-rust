---
"gws-rust": patch
---

Interactive `gwsr auth setup` no longer pre-fills the saved OAuth client ID when that client belongs to a different project than the one being set up. Before this fix, pressing Enter after switching projects saved the old project's client under the new project.
