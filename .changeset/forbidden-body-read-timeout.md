---
"gws-rust": patch
---
A 403 response whose body stops arriving now fails after `--timeout` (exit 10) instead of hanging forever while gwsr checks it for a rate-limit reason.
