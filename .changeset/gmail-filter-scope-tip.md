---
"gws-rust": patch
---

The `gmail +filter` help tip now names a login that grants the scope it needs: `gwsr auth login --scopes gmail.settings.basic`. It used to suggest `gwsr auth login -s gmail`, which requests only read-only Gmail scopes without a terminal, so `+filter` still failed for lack of `gmail.settings.basic`.
