---
"gws-rust": patch
---

`--format table` now measures cells in terminal columns instead of characters, so columns stay aligned after CJK text and emoji, the 60-column cap holds for wide characters, and truncation never cuts a flag, skin-tone emoji or accented letter in half.
