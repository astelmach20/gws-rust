---
"gws-rust": patch
---

`GWSR_RESTRICT_PATHS=cwd` no longer accepts a path such as `link/missing/../file` where `link` is a symlink pointing outside the current directory. `..` is now applied after symlinks in the existing prefix are resolved, so these paths are rejected (and, without the restriction, resolve to the symlink's real target).
