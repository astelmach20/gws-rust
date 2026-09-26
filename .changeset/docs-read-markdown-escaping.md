---
"gws-rust": patch
---

docs `+read` (Markdown, the default body format) now escapes document text that looks like Markdown. A paragraph such as `# of seats: 12`, `1. …`, `- 5 degrees` or `f(*args, **kwargs)` used to come out as a heading, a list or emphasis, and inline tags like `<b>` as raw HTML; they now read back as the literal text. Real bold, italic, links and code runs are still rendered as Markdown.
