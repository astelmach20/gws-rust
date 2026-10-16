---
"gws-rust": patch
---

Gmail `+search` and `+triage` skip a message that was deleted between listing and fetching (404) with a warning on stderr, instead of failing the whole run. The other results are printed and the command exits 0, as `+watch` does for a message that is gone. Any other failed fetch still fails the command.
