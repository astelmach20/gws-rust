---
"gws-rust": patch
---

`script +push` no longer deletes remote files that a local symlink stands for. It used to skip symlinks with a warning and then replace the whole project, so the remote copy of a symlinked `.gs`/`.js`/`.html` file (or of every file under a symlinked directory) was deleted. It now refuses the push and names those symlinks; replace each with a copy of its target. Symlinks whose names would not be pushed anyway (such as `README.md`) are still ignored.
