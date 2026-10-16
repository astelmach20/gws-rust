---
"gws-rust": patch
---

Tests only: the `drive +upload` path-confinement test now sets `GWSR_RESTRICT_PATHS=cwd` and uploads a file that exists. Before, it passed only because the file was missing.
