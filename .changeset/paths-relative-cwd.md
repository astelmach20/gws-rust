---
"gws-rust": patch
---

`gws-rust-core` path resolvers (`resolve_file_path`, `resolve_output_dir`, `resolve_dir_path`, `check_derived_path`) now resolve a relative `cwd` against the process working directory. Before, a leading `..` was dropped lexically, so `../x` with a `cwd` of `.` resolved to `x` (the wrong file) and the result was not absolute.
