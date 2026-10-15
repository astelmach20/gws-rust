---
"gws-rust": patch
---

`script +pull` now refuses a project file whose `source` is present but not a string (null, number, object, array). It names the file and writes nothing. Previously it wrote an empty local file, so a later `+push` (which replaces the whole project) could wipe that file. A file with no `source` at all is still pulled as an empty file, because the API omits empty strings.
