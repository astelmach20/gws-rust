---
"gws-rust": patch
---

Helper `--output PATH` (`drive +download`/`+export`, `docs +read`, `sheets +read`, `forms +responses`) refuses an existing file without `--overwrite` before loading credentials or sending any request (exit 3). Before, the refusal came only after the content was fetched, and without credentials the command failed with an auth error (exit 2) instead. `drive +download`/`+export` without `--output` also check the name taken from Drive before starting the download.
