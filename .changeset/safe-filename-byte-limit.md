---
"gws-rust": patch
---

`drive +download`, `drive +export` and `drive +sync` no longer fail with "File name too long" on Drive files with long non-ASCII names (emoji, CJK, accented text). Local names derived from Drive names are now capped at 200 bytes rather than 200 characters; previously one such file aborted the whole `+sync`, leaving the remaining files undownloaded.
