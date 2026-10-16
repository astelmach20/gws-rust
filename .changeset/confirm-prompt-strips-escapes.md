---
"gws-rust": patch
---

The interactive confirmation prompt (`About to … Proceed? [y/N]`) now strips control characters and escape sequences from the action it names. An argument such as `--event-id` or `--attendee` could otherwise rewrite the question on the terminal, or use OSC 52 to set the clipboard, before the user answered.
