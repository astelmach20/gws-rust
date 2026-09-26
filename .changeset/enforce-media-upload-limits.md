---
"gws-rust": patch
---

`--upload` now enforces the method's Discovery upload limits before sending anything: a file larger than `mediaUpload.maxSize` (binary units, e.g. 35 MiB for `gmail users messages send`) or with a media type outside `mediaUpload.accept` (e.g. `text/plain` where Gmail accepts only `message/*`) fails with a validation error (exit 3), also under `--dry-run`, instead of being uploaded in full and then rejected by the server. Set the media type with `--upload-content-type` when detection picks the wrong one. The Gmail helpers (`+send`, `+reply`, `+reply-all` and `+forward`, including drafts) likewise refuse an encoded message over Gmail's 35 MiB limit before sending it.
