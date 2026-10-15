---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` no longer lose a heading inside a list item that is followed by more of the item, as in `- # Title` then an indented paragraph. The heading's paragraph was dropped from the write plan, so its text was inserted without the heading style and the list around it was split in two.
