---
"gws-rust": patch
---

`drive +download` and `drive +export` of a shortcut now suggest the command that fits the target's type (`shortcutDetails.targetMimeType`): `+export` for a Google Doc, Sheet, Slides or Drawing (with the requested format only when it fits), `+download` for a regular file, and neither for a target that cannot be downloaded or exported. They used to echo the command that was run, so a shortcut to a Doc pointed at `+download` and one to a PDF at `+export`, both of which fail.
