---
"gws-rust": patch
---

`drive +sync` now fails with a validation error when `--folder-id` is a folder in the trash. Before, it listed nothing (everything in a trashed folder is trashed too) and reported success with an empty mirror.
