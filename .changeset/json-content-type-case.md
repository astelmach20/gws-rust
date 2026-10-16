---
"gws-rust": patch
---

Generated commands now recognize a JSON response whatever the letter case of its `Content-Type` (`Application/JSON; charset=UTF-8`, `TEXT/JSON`), as media types are case-insensitive. Such a response used to be refused as "binary" and needed `-o` to read.
