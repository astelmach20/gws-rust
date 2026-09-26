---
"gws-rust": patch
---

Helper `--dry-run` plans now list every query parameter the real request sends. `gmail +search --include-spam-trash` shows `includeSpamTrash`. A key the request repeats, such as `sources` in `people +find --directory`, is now shown as an array of all its values instead of only the last one.
