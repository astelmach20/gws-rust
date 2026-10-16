---
"gws-rust": patch
---

A `--jq` filter or output format that fails after a command already changed something (sent a message, trashed it, created a task, filter or Meet space, or ran a generated POST/PATCH/DELETE method) now says the change was made and names it, instead of a plain validation error that reads as "nothing happened" and invites a duplicate retry.
