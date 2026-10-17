---
"gws-rust": patch
---

`drive +sync` now checks each directory it creates for a Drive folder against `GWSR_RESTRICT_PATHS=cwd`, as it already checked every file it writes. Before, a Drive folder with no file to download, reached through a symlinked sub-directory of `--dir`, was created outside the current directory and the run succeeded. Now the run fails with a validation error (exit 3) and creates nothing there.
