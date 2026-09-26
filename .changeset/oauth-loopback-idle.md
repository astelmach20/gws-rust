---
"gws-rust": patch
---

`gwsr auth login` no longer stalls for about 10 seconds after you approve access in the browser. The local callback listener now serves connections concurrently (up to 16 at once), so an idle connection the browser opens ahead of time can no longer hold up the real redirect.
