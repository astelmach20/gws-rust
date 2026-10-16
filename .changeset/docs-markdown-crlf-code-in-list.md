---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` no longer add blank lines inside a code block that belongs to a list item when the Markdown has Windows (CRLF) line endings. Each code line used to be followed by an empty line, and the item ended with a stray line break.
