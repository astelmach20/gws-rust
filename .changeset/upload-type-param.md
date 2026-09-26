---
"gws-rust": patch
---

`--upload` now refuses an `uploadType` in `--params` with a validation error (exit 3). Previously both were sent (for example `uploadType=media&uploadType=multipart`), so the server could store the multipart envelope as the file's content. `--dry-run` with `--upload` now lists the `uploadType` that is actually sent in `query_params`.
