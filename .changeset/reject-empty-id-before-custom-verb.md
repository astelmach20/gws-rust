---
"gws-rust": patch
---

An empty ID in front of a custom method is now refused before anything is sent, real run and `--dry-run` alike. `docs +write --document-id ""`, `script +run --script-id ""` and `sheets +clear --range ""` used to send (or plan) `POST .../documents/:batchUpdate`, `.../scripts/:run` and `.../values/:clear` and fail at Google with an HTML 400 page; they now exit 3 with an "empty segment" validation error, like an empty ID anywhere else in the path.
