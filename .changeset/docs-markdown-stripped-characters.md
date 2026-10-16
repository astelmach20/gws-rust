---
"gws-rust": patch
---

`docs +write --markdown` and `docs +create --markdown` no longer misplace formatting after a control character (U+0000–U+0008, U+000C–U+001F) or a Private Use Area character (U+E000–U+F8FF). The Docs API drops these from inserted text, so every bold, italic, link, code, heading and list range after one landed on the wrong characters; they are now left out of the inserted text and its offsets.
