---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` accept Markdown that is a single empty paragraph, such as a lone `---` rule, an empty `#` heading or an empty `-` list item, when it lands in an empty last paragraph (always the case for `+create`). The request no longer starts with an `insertText` that has no text, which Docs rejects; the paragraph styles apply to the existing paragraph instead.
