---
"gws-rust": patch
---

`docs +read` Markdown output no longer corrupts text that Google Docs splits into several runs with the same bold, italic, strikethrough, code or link style: they are wrapped once (`*xy*`), not per run (`*x**y*`, which shows literal asterisks and can swallow inline code).
