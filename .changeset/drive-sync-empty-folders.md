---
"gws-rust": patch
---

`drive +sync` now creates a local directory for every Drive folder it mirrors. Before, a directory was only created when a file was downloaded into it, so empty folders, folders holding only empty sub-folders and folders of skipped items (Forms, shortcuts...) were missing from the mirror.
