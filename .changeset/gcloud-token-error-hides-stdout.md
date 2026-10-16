---
"gws-rust": patch
---
When `gcloud auth print-access-token` fails during `gwsr auth setup`, the error shows gcloud's stderr only, never a token it may already have printed to stdout.
