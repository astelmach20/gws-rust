---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` now link an email autolink such as `<ann@example.com>` to `mailto:ann@example.com`. Before, the link URL was the bare address, which is not a mail link.
