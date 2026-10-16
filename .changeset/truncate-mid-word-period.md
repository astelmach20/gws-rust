---
"gws-rust": patch
---

Shortened method descriptions in `--help` and generated skills no longer end at a period inside a word. When the length limit fell right after the period of `developers.google.com` or `v1.5`, the text was cut there as if a sentence ended (`See the docs at developers.`); it now falls back to the last whole word with `…`. A period followed by a line break now counts as a sentence end, like one followed by a space.
