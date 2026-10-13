---
"gws-rust": patch
---

Generated service skills now list each API method on one Markdown line. Discovery descriptions with paragraphs or bullet lists (calendar `calendars.insert`, `calendars.transferOwnership`, `events.import`) no longer break out of the method list, and a method without a description (script `scripts.run`) no longer leaves a dangling `—` and trailing whitespace.
