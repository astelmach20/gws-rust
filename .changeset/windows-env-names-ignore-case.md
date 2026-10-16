---
"gws-rust": patch
---
On Windows, `GWSR_*` and `RUST_LOG` environment variables are matched ignoring case like every other Windows program does, so `gwsr_restrict_paths=cwd` is applied (or rejected when invalid) instead of being silently ignored.
