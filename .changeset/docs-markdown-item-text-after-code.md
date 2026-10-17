---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` no longer glue a list item's text onto the last line of a code block before it. Text after a fenced or indented code block in the same item now starts on a new line of the item, as the code lines do; before, `x` followed by `after` came out as `xafter`.
