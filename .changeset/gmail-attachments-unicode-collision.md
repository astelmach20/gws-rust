---
"gws-rust": patch
---

`gmail +attachments` now keeps apart attachment names that differ only in Unicode normalization, such as a precomposed `café.pdf` and a decomposed `café.pdf`. macOS treats the two spellings as one file, so the second attachment used to replace the first under `--overwrite` (or fail halfway without it), while the output still listed both. The second one is now saved as `café (1).pdf`.
