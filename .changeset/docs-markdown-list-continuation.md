---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` keep multi-line list items in one bullet. A second paragraph inside a list item used to become an extra bullet (so `1. a` / `more` / `2. b` numbered three items), and a hard line break or code block inside an item split the list so the following items restarted at 1. These now continue the item on a new line of the same bullet.
