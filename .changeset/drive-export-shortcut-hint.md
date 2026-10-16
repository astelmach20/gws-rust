---
"gws-rust": patch
---

`drive +export` of a shortcut now names the shortcut's target (`+export --file-id TARGET --to FORMAT`) instead of sending you to `+download`, which refuses shortcuts. A Google-native type that cannot be exported (Forms, Sites, folders and so on) is reported as such, without the `+download` hint; regular files are still pointed at `+download`.
