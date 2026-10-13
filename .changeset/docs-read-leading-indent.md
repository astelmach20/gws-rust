---
"gws-rust": patch
---

`docs +read` Markdown output drops the leading tabs and spaces of a paragraph instead of emitting them, so indented paragraphs are no longer turned into code blocks that show escaped text (`snake\_case`) verbatim.
