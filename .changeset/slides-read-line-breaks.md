---
"gws-rust": patch
---

`slides +read` turns line breaks inside a paragraph (Shift+Enter, which Slides stores as a vertical tab) into newlines in `text` and `notes`, like `docs +read`. They were returned as U+000B, which terminal output drops as a control character, so the words on either side of the break ran together ("Q3 resultsAll regions").
