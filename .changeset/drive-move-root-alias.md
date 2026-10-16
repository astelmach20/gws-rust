---
"gws-rust": patch
---

`drive +move --folder-id root` now resolves `root` to the real My Drive folder ID before comparing it with the file's parents. A file already only in My Drive is refused with "File is already in folder root" instead of a request that adds and removes that same folder, and a file in My Drive and another folder only loses the other folder.
