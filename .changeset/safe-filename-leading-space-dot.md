---
"gws-rust": patch
---

Downloads named after remote files (`drive +download`, `+export`, `+sync`) no longer turn a remote name such as ` .bashrc` (whitespace before a leading dot) into a hidden local file: leading dots are now stripped together with any whitespace around them.
