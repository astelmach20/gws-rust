---
"gws-rust": patch
---

`drive +upload --convert` now refuses a file type no Google format imports while parsing its flags, before credentials load, so the mistake exits 3 (validation) instead of 2 (auth) when no one is signed in.
