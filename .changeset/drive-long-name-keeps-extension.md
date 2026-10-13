---
"gws-rust": patch
---

`drive +download` and `drive +sync` keep a file's extension when they shorten a long Drive name for the local file. Before, a name longer than 200 bytes was cut at the end, so `<long name>.pdf` was saved without `.pdf`; this also hit `+sync`'s ID-prefixed names for same-named files, including exported `.docx`/`.xlsx`/`.pptx` copies.
