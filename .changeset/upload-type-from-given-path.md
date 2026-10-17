---
"gws-rust": patch
---

`--upload` on generated methods and `drive +upload` now infer the content type from the path you give, not from the file a symlink points to, as `gmail --attach` already did. Before, `notes.csv -> blob` was sent as `application/octet-stream`, and `drive +upload --convert` refused it.
